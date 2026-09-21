import Foundation
import RadrootsKit
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraRecoveryInventoryTests: XCTestCase {
  func testGeneratedInventoryAndExactParentPreserveScopedOperationIdentity() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    try await runtime.configureSimulatorRelays(loopbackRelays: ["ws://127.0.0.1:19999"])
    let backend = TeraGeneratedRuntimeBackend(runtime: runtime)
    let scope = TeraComposerScope(authorPublicKey: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798", localNetworkID: "other-context")
    var form = TeraComposerForm(commandType: .createUpdate)
    form.content = "saved operation outside display selection"
    let source = try await backend.saveComposer(request: TeraComposerSaveRequest(
      scope: scope, id: backend.reserveComposerID(), expectedRevision: nil, editSequence: 1, form: form
    ))
    let request = try await TeraSubmissionRequest(commandID: backend.reserveSubmissionID(), scope: scope, composerID: source.draft.id, expectedRevision: 1)
    let status = try await backend.prepareSubmission(request: request, media: [])
    let exact = try await backend.recoveryParent(key: status.intentID)
    XCTAssertEqual(exact?.owner, .submission(request))
    let display = try await backend.draftHeads(limit: 100)
    XCTAssertTrue(display.isEmpty)
    var cursor: String?
    var entries: [TeraRecoveryEntry] = []
    var scanned = 0
    repeat {
      let page = try await backend.recoveryPage(limit: 1, cursor: cursor)
      XCTAssertEqual(page.author, scope.authorPublicKey)
      entries += page.entries
      scanned += Int(page.scanned)
      cursor = page.nextCursor
    } while cursor != nil
    XCTAssertEqual(scanned, 3)
    XCTAssertEqual(entries, [exact].compactMap(\.self))
    do {
      _ = try await runtime.recoveryPage(schemaVersion: 2, limit: 1, cursor: nil)
      XCTFail("Unknown inventory version must fail")
    } catch let TeraAppError.Failure(report) {
      XCTAssertEqual(report.code, "unsupported_schema_version")
    }
    _ = try await runtime.shutdown()
  }

  func testThousandNativeOwnersContinueBeyondDisplayLimitAndIsolateMissingParent() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    let verified = TeraNativeUploadRecoveryOwner(draft: fixture.draft(revision: 3, stage: .verified))
    for number in 1 ... 1000 {
      let identifier = try RadrootsBackgroundTransferIdentifier("radroots.add.\(Self.key(number)).2.\(String(repeating: "a", count: 32))")
      try await transfer.seed(request: TeraBackgroundUploadRequest.replacingIdentifier(in: request, with: identifier), state: .awaitingVerification)
    }
    let seen = RecoveryLookups()
    var cursor: String?
    var visited = 0
    var passes = 0
    repeat {
      let result = try await RecoverySettlementFixture.run(transfer: transfer, cursor: cursor) { key in
        await seen.append(key)
        if key == Self.key(1) {
          return nil
        }
        return Self.owner(key: key, template: verified)
      }
      XCTAssertLessThanOrEqual(result.progress.visited, 64)
      visited += result.progress.visited
      passes += 1
      cursor = result.cursor
      if passes == 1 {
        XCTAssertEqual(result.progress.remaining, 936)
      }
    } while cursor != nil
    XCTAssertEqual(visited, 1000)
    XCTAssertEqual(passes, 16)
    let keys = await seen.keys
    XCTAssertEqual(Set(keys), Set((1 ... 1000).map(Self.key)))
    XCTAssertEqual(keys.count, 1000)
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 999)
    XCTAssertEqual(counts.enqueue, 0)
    XCTAssertEqual(counts.retry, 0)
    let fresh = try await RecoverySettlementFixture.run(transfer: transfer, cursor: nil) { key in
      Self.owner(key: key, template: verified)
    }
    XCTAssertEqual(fresh.progress, .init(visited: 1, remaining: 0, needsAttention: false))
    let finalCounts = await transfer.counts
    XCTAssertEqual(finalCounts.acceptedSettlement, 1000)
  }

  func testCancelledExactLookupRetainsReceiptAndFreshPassRecovers() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    try await transfer.seed(request: request, state: .awaitingVerification)
    let pause = ResourceTestPause()
    let owner = TeraNativeUploadRecoveryOwner(draft: fixture.draft(revision: 3, stage: .verified))
    let task = Task {
      try await RecoverySettlementFixture.run(transfer: transfer, cursor: nil) { _ in
        await pause.wait()
        return owner
      }
    }
    await pause.entered.wait()
    task.cancel()
    await pause.resume.open()
    do { _ = try await task.value; XCTFail("Cancelled pass must not settle") } catch is CancellationError {}
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 0)
    let result = try await RecoverySettlementFixture.run(transfer: transfer, cursor: nil) { _ in owner }
    XCTAssertEqual(result.progress, .init(visited: 1, remaining: 0, needsAttention: false))
  }

  func testMismatchedExactParentRemainsUnsettled() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    try await transfer.seed(request: fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32))), state: .awaitingVerification)
    let owner = TeraNativeUploadRecoveryOwner(draft: fixture.draft(revision: 3, stage: .verified))
    let result = try await RecoverySettlementFixture.run(transfer: transfer, cursor: nil) { _ in
      Self.owner(key: Self.key(7), template: owner)
    }
    XCTAssertTrue(result.progress.needsAttention)
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 0)
    XCTAssertEqual(counts.cancel, 0)
  }

  private static func key(_ value: Int) -> String {
    String(format: "%032x", value)
  }

  private static func owner(key: String, template: TeraNativeUploadRecoveryOwner) -> TeraNativeUploadRecoveryOwner {
    .init(id: key, revision: template.revision, media: template.media, verifiedURLs: template.verifiedURLs, uploadURLs: template.uploadURLs)
  }
}

private actor RecoveryLookups {
  private(set) var keys: [String] = []
  func append(_ key: String) {
    keys.append(key)
  }
}
