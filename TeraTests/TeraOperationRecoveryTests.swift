import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraOperationRecoveryTests: XCTestCase {
  @MainActor
  func testSelectedTransferUsesExactKeyWithoutMovingSweepOrAdmittingSecondWorker() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let selected = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    let other = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "b", count: 32)))
    try await transfer.seed(request: selected, state: .awaitingVerification)
    try await transfer.seed(request: other, state: .awaitingVerification)
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let coordinator = fixture.coordinator(transfer: transfer)
    let prior = try await client.nativeRecoverySchedule()
    let key = TeraNativeRecoveryIssue.key(selected.identifier.rawValue)
    let pause = ResourceTestPause()
    await transfer.pauseDiscovery(pause)
    let task = Task { try await coordinator.recoverNativeUpload(key: key, client: client) }
    await pause.entered.wait()
    do {
      _ = try await coordinator.recoverNativeUploads(client: client)
      XCTFail("Selected check must retain the same recovery admission")
    } catch {}
    await pause.resume.open()
    let result = try await task.value
    XCTAssertEqual(result.visited, 1)
    XCTAssertEqual(result.remaining, 0)
    XCTAssertEqual(result.issues.map(\.key), [key])
    let after = try await client.nativeRecoverySchedule()
    XCTAssertEqual(after, prior)
    for absent in [String(repeating: "0", count: 64), "invalid"] {
      do {
        _ = try await coordinator.recoverNativeUpload(key: absent, client: client)
        XCTFail("An absent or invalid selected record is not success")
      } catch {}
    }
    let counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 0)
    XCTAssertEqual(counts.acceptedSettlement, 0)
    _ = try await client.stop()
  }

  func testCompletedTransferRepairsLostAdvisoryWriteAfterRuntimeReopen() async throws {
    let fixture = try BackgroundUploadFixture()
    let storage = try MediaOwnershipFixture()
    defer { fixture.remove(); storage.remove() }
    let transfer = BackgroundTransferHarness()
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    try await transfer.seed(request: request, state: .awaitingVerification)
    let key = TeraNativeRecoveryIssue.key(request.identifier.rawValue)
    let first = try await storage.runtime()
    let backend = TeraGeneratedRuntimeBackend(runtime: first)
    _ = try await backend.reportNativeRecoveryStatus(key: key, reason: .outcomeUnconfirmed)
    let owner = TeraNativeUploadRecoveryOwner(draft: fixture.draft(revision: 3, stage: .verified))
    // Settle native evidence, then model the lost advisory write by retaining it.
    _ = try await RecoverySettlementFixture.run(transfer: transfer, cursor: nil) { _ in owner }
    _ = try await first.shutdown()
    let reopened = try await storage.runtime()
    let next = TeraGeneratedRuntimeBackend(runtime: reopened)
    let inspection = TeraNativeRecoveryInspection(completedNeedsRepair: { key in
      let status = try await next.nativeRecoveryStatus(key: key)
      return status != nil && status?.reason != .resolved
    })
    let result = try await TeraNativeRecoveryInventory.run(transfer: transfer, cursor: nil, inspection: inspection, complete: { snapshot, exactOwner in
      let input = try TeraRecoveryUploadReceipt(snapshot: snapshot, owner: exactOwner)
      try await TeraNativeUploadReconciliation.settle(snapshot, input: input, receipt: RecoverySettlementFixture.receipt(input), transfer: transfer)
    }, report: { snapshot, reason in
      let key = TeraNativeRecoveryIssue.key(snapshot.identifier.rawValue)
      let status = try? await next.reportNativeRecoveryStatus(key: key, reason: reason)
      return .init(key: key, reason: reason, status: status)
    }, lookup: { _ in owner })
    XCTAssertEqual(result.progress.issues.map(\.reason), [.resolved])
    let saved = try await next.nativeRecoveryStatus(key: key)
    XCTAssertEqual(saved?.reason, .resolved)
    XCTAssertEqual(saved?.revision, 2)
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 1)
    XCTAssertEqual(counts.enqueue, 0)
    XCTAssertEqual(counts.retry, 0)
    _ = try await reopened.shutdown()
  }

  func testCompletedEvidenceStillRequiresExactParentAndAssociation() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    try await transfer.seed(request: request, state: .completed)
    let inspection = TeraNativeRecoveryInspection(completedNeedsRepair: { _ in true })
    let missing = try await TeraNativeRecoveryInventory.run(transfer: transfer, cursor: nil, inspection: inspection) { _ in nil }
    XCTAssertEqual(missing.progress.issues.map(\.reason), [.missingParent])
    let mismatch = try await TeraNativeRecoveryInventory.run(transfer: transfer, cursor: nil, inspection: inspection, complete: { _, _ in
      throw TeraNativeRecoveryFault.associationMismatch
    }) { _ in .init(draft: fixture.draft(revision: 3, stage: .verified)) }
    XCTAssertEqual(mismatch.progress.issues.map(\.reason), [.associationMismatch])
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 0)
    XCTAssertEqual(counts.cancel, 0)
  }

  @MainActor
  func testFailedResolvedReportCannotClearNoticeAndPauseReasonsRemainDistinct() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let result = await TeraNativeRecoveryClassification.report("retained", reason: .resolved, client: client)
    XCTAssertNil(result)
    let saved = TeraNativeRecoveryStatus(key: TeraNativeRecoveryIssue.key("retained"), reason: .outcomeUnconfirmed,
                                         revision: 1, firstObservedUnixMS: 1, updatedAtUnixMS: 1)
    await backend.setNativeRepairValue(saved)
    let unresolved = await TeraNativeRecoveryClassification.report("retained", reason: .resolved, client: client)
    XCTAssertEqual(unresolved, .init(key: saved.key, reason: saved.reason, status: saved), "A fresh store must see a retained advisory when resolution fails")
    let reasons: [(String, TeraNativeRecoveryPause)] = [
      ("protected_data_unavailable", .protectedData), ("today_media_quota_exceeded", .quota),
      ("identity_unavailable", .credentials), ("today_runtime_unavailable", .runtimeUnavailable),
      ("store_path_unavailable", .storageUnavailable),
    ]
    for (code, pause) in reasons {
      let failure = TeraRuntimeFailure.local(operation: "test.recovery", code: code, safeMessage: "Paused.")
      XCTAssertEqual(TeraNativeRecoveryClassification.pause(failure), pause)
    }
    _ = try await client.stop()
  }
}
