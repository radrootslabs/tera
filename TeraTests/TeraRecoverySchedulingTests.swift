import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraRecoverySchedulingTests: XCTestCase {
  func testLifecycleBudgetContinuesUntilExhaustedAndNextResumeTraversesRemainder() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let media = RecoverySchedulingMedia(pending: 1000)
    let store = TeraNativeRepairStore(client: client, media: media)
    for expected in [744, 488, 232, 0] {
      await store.reconcile()
      XCTAssertEqual(store.progress?.remaining, expected)
      XCTAssertLessThanOrEqual(store.issues.count, 64)
    }
    let calls = await media.calls
    XCTAssertEqual(calls, 16)
    store.stop()
    _ = try await client.stop()
  }

  func testStopRetainsSingleWorkerAndCoalescesResumeWithoutFreshNotification() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let media = RecoverySchedulingMedia(pending: 1000)
    let pause = ResourceTestPause()
    await media.pauseNext(pause)
    let store = TeraNativeRepairStore(client: client, media: media)
    let running = Task { await store.reconcile() }
    await pause.entered.wait()
    for _ in 0 ..< 100 {
      store.retry()
    }
    store.stop()
    for _ in 0 ..< 100 {
      store.retry()
    }
    let during = await media.calls
    XCTAssertEqual(during, 1)
    XCTAssertTrue(store.isRunning)
    await pause.resume.open()
    _ = await running.value
    await TeraScopeFixtures.eventually { !store.isRunning }
    let final = await media.calls
    let maximum = await media.maximumActive
    XCTAssertEqual(final, 5, "One cancelled worker and one four-batch resumed request")
    XCTAssertEqual(maximum, 1)
    XCTAssertEqual(store.progress?.remaining, 744)
    store.stop()
    _ = try await client.stop()
  }

  func testForegroundResumeRetriesProtectedDataWithoutTransferOrDraftEvent() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let media = RecoverySchedulingMedia(pending: 65, locked: true)
    let stores = TeraProductStores(runtimeClient: client, addMedia: media)
    stores.configure(snapshot: TeraScopeFixtures.snapshot())
    await stores.resume()
    XCTAssertEqual(stores.add.recovery.transfers.progress?.pause, .protectedData)
    XCTAssertEqual(stores.add.state, .ready)
    await media.unlock()
    await stores.resume()
    XCTAssertNil(stores.add.recovery.transfers.progress?.pause)
    XCTAssertEqual(stores.add.recovery.transfers.progress?.remaining, 0)
    let calls = await media.calls
    XCTAssertEqual(calls, 3)
    stores.suspend()
    await stores.resume()
    XCTAssertEqual(stores.add.state, .ready)
    stores.stop()
    _ = try await client.stop()
  }

  func testAlreadyCancelledCallerCannotCancelCurrentRecovery() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let media = RecoverySchedulingMedia(pending: 65)
    let held = ResourceTestPause()
    await media.pauseNext(held)
    let store = TeraNativeRepairStore(client: client, media: media)
    let current = Task { await store.reconcile() }
    await held.entered.wait()
    let stalePause = ResourceTestPause()
    var staleReturned = false
    let stale = Task {
      await stalePause.wait()
      await store.reconcile()
      staleReturned = true
    }
    await stalePause.entered.wait()
    stale.cancel()
    await stalePause.resume.open()
    await TeraScopeFixtures.eventually { staleReturned }
    XCTAssertTrue(store.isRunning)
    await held.resume.open()
    _ = await current.value
    await stale.value
    XCTAssertEqual(store.progress?.remaining, 0)
    XCTAssertFalse(store.isRunning)
    store.stop()
    _ = try await client.stop()
  }
}

private actor RecoverySchedulingMedia: TeraAddMediaHandling {
  private var pending: Int
  private var locked: Bool
  private var pause: ResourceTestPause?
  private var active = 0
  private(set) var calls = 0
  private(set) var maximumActive = 0

  init(pending: Int, locked: Bool = false) {
    self.pending = pending; self.locked = locked
  }

  func pauseNext(_ value: ResourceTestPause) {
    pause = value
  }

  func unlock() {
    locked = false
  }

  func support() -> TeraAddMediaSupport {
    .unavailable
  }

  func recoverNativeUploads(client _: TeraRuntimeClient) async throws -> TeraNativeRecoveryProgress {
    calls += 1
    active += 1
    maximumActive = max(maximumActive, active)
    defer { active -= 1 }
    if let pause {
      self.pause = nil; await pause.wait()
    }
    try Task.checkCancellation()
    if locked {
      return .init(visited: 0, remaining: pending, needsAttention: false, pause: .protectedData)
    }
    let visited = min(64, pending)
    pending -= visited
    return .init(visited: visited, remaining: pending, needsAttention: false)
  }

  func importImages(limit _: Int) throws -> [TeraPreparedMedia] {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func captureImage() throws -> TeraPreparedMedia {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func open(_: [TeraPreparedMedia]) throws -> TeraOpenedMedia {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}
