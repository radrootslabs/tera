@testable import TeraApp
import XCTest

@MainActor
final class TeraRestoreRecoveryStoreTests: XCTestCase {
  func testStatusAndReviewNeverResumeWithoutTheSeparateAction() async throws {
    let backend = try TeraScopeBackend()
    await backend.restoreStorage.configure(targets: [])
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraRestoreRecoveryStore(client: client)
    await store.load()
    XCTAssertEqual(store.status?.phase, .held)
    XCTAssertNil(store.reviewedInventory)
    await store.review()
    XCTAssertEqual(store.status?.phase, .reviewed)
    XCTAssertEqual(store.reviewedInventory, "reviewed")
    let before = await backend.restoreStorage.calls
    XCTAssertEqual(before, ["review"])
    await store.resume()
    XCTAssertEqual(store.status?.phase, .resumed)
    XCTAssertNil(store.reviewedInventory)
    let after = await backend.restoreStorage.calls
    XCTAssertEqual(after, ["review", "resume"])
    _ = try await client.stop()
  }

  func testOneActionChecksOneDestinationAndDoesNotStarveUncheckedWork() async throws {
    let backend = try TeraScopeBackend()
    await backend.restoreStorage.configure(targets: [
      TeraRestoreTarget(draftID: "offline", eventID: "a", targetFingerprint: "first", observation: .incomplete),
      TeraRestoreTarget(draftID: "unchecked", eventID: "b", targetFingerprint: "second", observation: nil),
    ])
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraRestoreRecoveryStore(client: client)
    await store.load()
    await store.checkNext()
    let first = await backend.restoreStorage.calls
    XCTAssertEqual(first, ["check:unchecked"])
    XCTAssertNil(store.reviewedInventory)
    await store.checkNext()
    let second = await backend.restoreStorage.calls
    XCTAssertEqual(second, ["check:unchecked", "check:offline"])
    _ = try await client.stop()
  }

  func testAccountInvalidationDiscardsLateReviewAndCannotExposeResume() async throws {
    let backend = try TeraScopeBackend()
    let pause = ResourceTestPause()
    await backend.restoreStorage.configure(targets: [], pause: pause)
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraRestoreRecoveryStore(client: client)
    await store.load()
    let task = Task { await store.review() }
    await pause.entered.wait()
    store.invalidate()
    await pause.resume.open()
    await task.value
    XCTAssertNil(store.status)
    XCTAssertNil(store.reviewedInventory)
    await store.resume()
    let calls = await backend.restoreStorage.calls
    XCTAssertEqual(calls, ["review"])
    _ = try await client.stop()
  }
}

actor RestoreTestStorage {
  private var value: TeraRestoreStatus?
  private var pause: ResourceTestPause?
  private(set) var calls: [String] = []
  func configure(targets: [TeraRestoreTarget], pause: ResourceTestPause? = nil) {
    value = TeraRestoreStatus(attemptID: "fixture", phase: .held, targets: targets)
    self.pause = pause
  }

  func status() -> TeraRestoreStatus? {
    value
  }

  func check(_ target: TeraRestoreTarget) {
    calls.append("check:\(target.draftID)")
    guard let value else { return }
    self.value = TeraRestoreStatus(attemptID: value.attemptID, phase: value.phase, targets: value.targets.map {
      $0 == target ? TeraRestoreTarget(draftID: $0.draftID, eventID: $0.eventID, targetFingerprint: $0.targetFingerprint, observation: .notObserved) : $0
    })
  }

  func review() async throws -> String {
    calls.append("review")
    if let pause {
      await pause.wait()
    }
    guard let value else { throw TeraComposerAcknowledgment.unconfirmed }
    self.value = TeraRestoreStatus(attemptID: value.attemptID, phase: .reviewed, targets: value.targets)
    return "reviewed"
  }

  func resume(_ digest: String) throws {
    guard digest == "reviewed", let value, value.phase == .reviewed else { throw TeraComposerAcknowledgment.unconfirmed }
    calls.append("resume")
    self.value = TeraRestoreStatus(attemptID: value.attemptID, phase: .resumed, targets: value.targets)
  }
}

extension TeraScopeBackend {
  func restoreStatus() async -> TeraRestoreStatus? {
    await restoreStorage.status()
  }

  func reconcileRestoredTarget(_ target: TeraRestoreTarget) async {
    await restoreStorage.check(target)
  }

  func reviewRestoredWork() async throws -> String {
    try await restoreStorage.review()
  }

  func resumeRestoredWork(reviewedInventory: String) async throws {
    try await restoreStorage.resume(reviewedInventory)
  }
}
