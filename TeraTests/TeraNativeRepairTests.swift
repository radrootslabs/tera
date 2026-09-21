import Foundation
import RadrootsKit
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraNativeRepairTests: XCTestCase {
  func testMissingAndMismatchedTransfersPersistIndependentlyOfValidSettlement() async throws {
    let fixture = try BackgroundUploadFixture()
    let storage = try MediaOwnershipFixture()
    defer { fixture.remove(); storage.remove() }
    let runtime = try await storage.runtime()
    let backend = TeraGeneratedRuntimeBackend(runtime: runtime)
    let transfer = BackgroundTransferHarness()
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    let missing = try RadrootsBackgroundTransferIdentifier("radroots.add.\(String(repeating: "0", count: 31))1.2.\(String(repeating: "a", count: 32))")
    try await transfer.seed(request: TeraBackgroundUploadRequest.replacingIdentifier(in: request, with: missing), state: .awaitingVerification)
    try await transfer.seed(request: request, state: .awaitingVerification)
    let mismatch = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "b", count: 32)), remoteURL: "http://127.0.0.1:3000/changed")
    try await transfer.seed(request: mismatch, state: .awaitingVerification)
    let owner = TeraNativeUploadRecoveryOwner(draft: fixture.draft(revision: 3, stage: .verified))
    let run = { @Sendable in
      try await TeraNativeRecoveryInventory.run(transfer: transfer, cursor: nil, complete: { snapshot, owner in
        let input = try TeraRecoveryUploadReceipt(snapshot: snapshot, owner: owner)
        try await TeraNativeUploadReconciliation.settle(snapshot, input: input, receipt: RecoverySettlementFixture.receipt(input), transfer: transfer)
      }, report: { snapshot, reason in
        let key = TeraNativeRecoveryIssue.key(snapshot.identifier.rawValue)
        let status = try? await backend.reportNativeRecoveryStatus(key: key, reason: reason)
        return reason == .resolved ? nil : .init(key: key, reason: reason, status: status)
      }, lookup: { key in key == fixture.draftID ? owner : nil })
    }
    let first = try await run()
    XCTAssertEqual(first.progress.visited, 3)
    XCTAssertEqual(first.progress.issues.count, 2)
    XCTAssertTrue(first.progress.issues.contains { $0.reason == .missingParent })
    XCTAssertTrue(first.progress.issues.contains { $0.reason == .associationMismatch })
    XCTAssertTrue(first.progress.issues.allSatisfy { $0.status?.revision == 1 })
    let replay = try await run()
    XCTAssertEqual(replay.progress.visited, 2)
    XCTAssertEqual(replay.progress.issues, first.progress.issues)
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 1)
    XCTAssertEqual(counts.cancel, 0)
    XCTAssertEqual(counts.enqueue, 0)
    for issue in first.progress.issues {
      let saved = try await backend.nativeRecoveryStatus(key: issue.key)
      XCTAssertEqual(saved, issue.status)
    }
    _ = try await runtime.shutdown()
  }

  func testProtectedDataPausesAtSameCursorWithoutCorruptionOrSettlement() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    try await transfer.seed(request: fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32))), state: .awaitingVerification)
    let failure = TeraRuntimeFailure.local(operation: "test.recovery", code: "protected_data_unavailable", safeMessage: "Locked.")
    let result = try await TeraNativeRecoveryInventory.run(transfer: transfer, cursor: nil) { _ in throw failure }
    XCTAssertEqual(result.progress.pause, .protectedData)
    XCTAssertEqual(result.progress.visited, 0)
    XCTAssertEqual(result.progress.remaining, 1)
    XCTAssertFalse(result.progress.needsAttention)
    XCTAssertTrue(result.progress.issues.isEmpty)
    XCTAssertNil(result.cursor)
    let state = await transfer.state
    XCTAssertEqual(state, .awaitingVerification)
  }

  @MainActor
  func testRunningNativeRecoveryCannotBlockUnrelatedSaveAndSubmit() async throws {
    let backend = AddBackend(advanceOffline: true)
    let client = try await TeraAddStoreTests.startedClient(backend)
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let pause = ResourceTestPause()
    await transfer.pauseDiscovery(pause)
    let store = TeraAddStore(runtimeClient: client, media: fixture.coordinator(transfer: transfer))
    await store.configure(snapshot: backend.snapshot())
    let startup = Task { await store.start() }
    await pause.entered.wait()
    XCTAssertEqual(store.state, .ready)
    store.updateForm(\.content, "An unrelated saved update")
    await store.save()
    XCTAssertEqual(store.composerState, .saved)
    await store.submit()
    XCTAssertEqual(store.submissions.status?.state, .queued)
    XCTAssertTrue(store.recovery.transfers.isRunning)
    await pause.resume.open()
    await startup.value
    XCTAssertEqual(store.state, .ready)
    store.stop()
    _ = try await client.stop()
  }

  @MainActor
  func testFailedStatusPersistenceIsNotReportedAsDurable() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let issue = await TeraNativeRecoveryClassification.report("unknown-native-record", reason: .missingParent, client: client)
    XCTAssertEqual(issue?.reason, .missingParent)
    XCTAssertNil(issue?.status)
    XCTAssertEqual(TeraNativeRecoveryClassification.pause(RadrootsBackgroundTransferError.persistenceFailure), .storageUnavailable)
    _ = try await client.stop()
  }

  func testGeneratedStatusRejectsUnknownVersionAndNoncanonicalIdentity() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    for (version, key) in [(UInt16(2), String(repeating: "a", count: 64)), (1, String(repeating: "A", count: 64))] {
      do {
        _ = try await runtime.reportNativeRecoveryStatus(schemaVersion: version, transferKey: key, reason: .missingParent)
        XCTFail("Invalid status input must fail before persistence")
      } catch let TeraAppError.Failure(report) {
        XCTAssertEqual(report.code, "invalid_native_recovery_status")
      }
    }
    let absent = try await runtime.nativeRecoveryStatus(schemaVersion: 1, transferKey: String(repeating: "a", count: 64))
    XCTAssertNil(absent)
    _ = try await runtime.shutdown()
  }
}
