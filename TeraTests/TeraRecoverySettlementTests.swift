import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

/// Fixture evidence only. Production obtains this receipt from Rust.
enum RecoverySettlementFixture {
  static func receipt(_ input: TeraRecoveryUploadReceipt) -> TeraRecoveryCompletionReceipt {
    .init(parent: input.parent, attempt: input.attempt, canonicalURL: input.media.remoteURL ?? "",
          sha256: input.media.sha256, mediaType: input.media.mediaType, byteSize: input.media.byteSize, verifiedAtUnixMS: 1_800_000_000_000)
  }

  static func run(transfer: BackgroundTransferHarness, cursor: String?,
                  lookup: @Sendable (String) async throws -> TeraNativeUploadRecoveryOwner?) async throws
    -> (progress: TeraNativeRecoveryProgress, cursor: String?)
  {
    try await TeraNativeRecoveryInventory.run(transfer: transfer, cursor: cursor, complete: { snapshot, owner in
      let input = try TeraRecoveryUploadReceipt(snapshot: snapshot, owner: owner)
      try await TeraNativeUploadReconciliation.settle(snapshot, input: input, receipt: receipt(input), transfer: transfer)
    }, lookup: lookup)
  }
}

final class TeraRecoverySettlementTests: XCTestCase {
  @MainActor
  func testVerifiedRustDraftReconcilesAwaitingReceiptAfterRelaunch() async throws {
    for legacyPath in [false, true] {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    try await transfer.seed(request: fixture.request(job: job, remoteURL: legacyPath ? job.remoteURL : nil),
                            state: .awaitingVerification)

    let backend = try TeraScopeBackend()
    let draft = fixture.draft(revision: 3, stage: .verified)
    await backend.setDrafts([draft])
    await backend.setRecoveryCompletion { input, _ in
      XCTAssertEqual(input.revision, 2)
      XCTAssertEqual(input.uploadURL, legacyPath ? job.remoteURL : job.uploadURL)
      return RecoverySettlementFixture.receipt(input)
    }
    let client = try await TeraScopeFixtures.client(backend)
    try await coordinator.reconcileBackgroundUploads(drafts: [draft], client: client)
    _ = try await client.stop()

    let counts = await transfer.counts
    let state = await transfer.state
    XCTAssertEqual(counts.acceptedSettlement, 1)
    XCTAssertEqual(state, .completed)
    }
  }

  @MainActor
  func testDuplicateDisplayHintsCannotDuplicateSettlement() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let draft = fixture.draft(revision: 3, stage: .verified)
    try await transfer.seed(request: fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32))), state: .awaitingVerification)
    let backend = try TeraScopeBackend()
    await backend.setDrafts([draft])
    await backend.setRecoveryCompletion { input, _ in RecoverySettlementFixture.receipt(input) }
    let client = try await TeraScopeFixtures.client(backend)
    try await coordinator.reconcileBackgroundUploads(drafts: [draft, draft], client: client)
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 1)
    _ = try await client.stop()
  }

  func testDuplicateAndLostSettlementReplyConvergeWithoutAnotherWrite() async throws {
    for lostReply in [false, true] {
      let fixture = try BackgroundUploadFixture()
      defer { fixture.remove() }
      let transfer = BackgroundTransferHarness()
      let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
      try await transfer.seed(request: request, state: .awaitingVerification)
      let value = try await transfer.snapshot(for: request.identifier)
      let snapshot = try XCTUnwrap(value)
      let input = try TeraRecoveryUploadReceipt(snapshot: snapshot, owner: .init(draft: fixture.draft(revision: 3, stage: .verified)))
      if lostReply {
        await transfer.failNextSettlement(afterWrite: true)
      }
      for _ in 0 ..< 2 {
        try await TeraNativeUploadReconciliation.settle(snapshot, input: input, receipt: RecoverySettlementFixture.receipt(input), transfer: transfer)
      }
      let counts = await transfer.counts
      XCTAssertEqual(counts.acceptedSettlement, 1)
      XCTAssertEqual(counts.enqueue, 0)
      XCTAssertEqual(counts.retry, 0)
    }
  }

  func testUnconfirmedSettlementRetainsReceiptAndCanResume() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    try await transfer.seed(request: request, state: .awaitingVerification)
    let value = try await transfer.snapshot(for: request.identifier)
    let snapshot = try XCTUnwrap(value)
    let input = try TeraRecoveryUploadReceipt(snapshot: snapshot, owner: .init(draft: fixture.draft(revision: 3, stage: .verified)))
    await transfer.failNextSettlement(afterWrite: false)
    do {
      try await TeraNativeUploadReconciliation.settle(snapshot, input: input, receipt: RecoverySettlementFixture.receipt(input), transfer: transfer)
      XCTFail("A lost reply without completed native state cannot succeed")
    } catch {}
    let state = await transfer.state
    XCTAssertEqual(state, .awaitingVerification)
    try await TeraNativeUploadReconciliation.settle(snapshot, input: input, receipt: RecoverySettlementFixture.receipt(input), transfer: transfer)
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 1)
  }

  func testChangedNativeAssociationCannotSettle() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    try await transfer.seed(request: request, state: .awaitingVerification)
    let value = try await transfer.snapshot(for: request.identifier)
    let snapshot = try XCTUnwrap(value)
    let input = try TeraRecoveryUploadReceipt(snapshot: snapshot, owner: .init(draft: fixture.draft(revision: 3, stage: .verified)))
    let changed = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)), remoteURL: "http://127.0.0.1:3000/changed")
    try await transfer.seed(request: changed, state: .awaitingVerification)
    do {
      try await TeraNativeUploadReconciliation.settle(snapshot, input: input, receipt: RecoverySettlementFixture.receipt(input), transfer: transfer)
      XCTFail("Changed request must retain evidence")
    } catch {}
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 0)
    XCTAssertEqual(counts.cancel, 0)
  }
}
