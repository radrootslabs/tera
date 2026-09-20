import Foundation
import RadrootsKit
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraRecoveryCompletionTests: XCTestCase {
  @MainActor
  func testLegacyCompletionErrorsPreserveNativeEvidenceForExactRecovery() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let transfer = BackgroundTransferHarness()
    let opened = try await fixture.coordinator(transfer: transfer).open([fixture.media])
    defer { opened.close() }
    let handle = try XCTUnwrap(opened.handles.first)
    let media = AddMediaHarness()
    let response = TeraAddBackgroundUploadReceipt(identifier: "retained", draftID: fixture.draftID, expectedRevision: 2,
                                                  statusCode: 200, mediaType: "application/json", contentEncoding: nil, body: Data("{}".utf8))
    for code in ["ios.runtime.timeout", "storage_failure", "invalid_native_upload_completion"] {
      await backend.failLegacyCompletion(with: .local(operation: "test.legacy", code: code, safeMessage: "Unconfirmed completion."))
      do {
        _ = try await TeraAddUploadCompletion.complete(response, handle: handle, media: media, runtimeClient: client)
        XCTFail("Completion should fail without settling")
      } catch {}
      let settlements = await media.settlementValues()
      XCTAssertEqual(settlements, [])
    }
    _ = try await client.stop()
  }

  @MainActor
  func testPendingReceiptWaitsForDurableCompletionBeforeSettlement() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    try await transfer.seed(request: fixture.request(job: job), state: .awaitingVerification)
    let backend = try TeraScopeBackend()
    await backend.setDrafts([job.draft])
    let pause = ResourceTestPause()
    await backend.setRecoveryCompletion { input, handle in
      XCTAssertEqual(handle.media, fixture.media)
      XCTAssertEqual(input.revision, 2)
      XCTAssertEqual(input.attempt, job.operationID)
      await pause.wait()
      return Self.verified(input)
    }
    let client = try await TeraScopeFixtures.client(backend)
    let coordinator = fixture.coordinator(transfer: transfer)
    let task = Task { try await coordinator.recoverNativeUploads(client: client) }
    await pause.entered.wait()
    let pending = await transfer.counts
    XCTAssertEqual(pending.acceptedSettlement, 0)
    await pause.resume.open()
    let progress = try await task.value
    XCTAssertEqual(progress, .init(visited: 1, remaining: 0, needsAttention: false))
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 1)
    XCTAssertEqual(counts.enqueue, 0)
    XCTAssertEqual(counts.retry, 0)
    _ = try await client.stop()
  }

  @MainActor
  func testCancelledCompletionRetainsNativeReceiptAndRestartCompletesWithoutUpload() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    try await transfer.seed(request: fixture.request(job: job), state: .awaitingVerification)
    let backend = try TeraScopeBackend()
    await backend.setDrafts([job.draft])
    let pause = ResourceTestPause()
    await backend.setRecoveryCompletion { input, _ in
      await pause.wait()
      return Self.verified(input)
    }
    let client = try await TeraScopeFixtures.client(backend)
    let coordinator = fixture.coordinator(transfer: transfer)
    let task = Task { try await coordinator.recoverNativeUploads(client: client) }
    await pause.entered.wait()
    task.cancel()
    await pause.resume.open()
    do { _ = try await task.value; XCTFail("Cancelled recovery must retain the receipt") } catch is CancellationError {}
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 0)
    await backend.setRecoveryCompletion { input, _ in Self.verified(input) }
    let restarted = fixture.coordinator(transfer: transfer)
    let progress = try await restarted.recoverNativeUploads(client: client)
    XCTAssertFalse(progress.needsAttention)
    let final = await transfer.counts
    XCTAssertEqual(final.acceptedSettlement, 1)
    XCTAssertEqual(final.enqueue, 0)
    XCTAssertEqual(final.retry, 0)
    _ = try await client.stop()
  }

  @MainActor
  func testUnconfirmedRustReceiptCannotSettleNativeEvidence() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    try await transfer.seed(request: fixture.request(job: job), state: .awaitingVerification)
    let backend = try TeraScopeBackend()
    await backend.setDrafts([job.draft])
    await backend.setRecoveryCompletion { input, _ in
      .init(parent: String(repeating: "b", count: 32), attempt: input.attempt, canonicalURL: input.media.remoteURL ?? "",
            sha256: input.media.sha256, mediaType: input.media.mediaType, byteSize: input.media.byteSize, verifiedAtUnixMS: 1)
    }
    let client = try await TeraScopeFixtures.client(backend)
    let result = try await fixture.coordinator(transfer: transfer).recoverNativeUploads(client: client)
    XCTAssertTrue(result.needsAttention)
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 0)
    XCTAssertEqual(counts.cancel, 0)
    _ = try await client.stop()
  }

  func testGeneratedCompletionRejectsUnsupportedNativeEvidenceBeforeMediaConversion() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let opened = try TeraMediaFileFixture.open([fixture.media], bytes: fixture.bytes)
    defer { opened.close() }
    var media = try XCTUnwrap(opened.handles.first).generatedValue
    media.schemaVersion = 0 // If converted first, this would return a different error.
    let receipt = FfiRecoveryUploadReceipt(schemaVersion: 2, parent: String(repeating: "1", count: 32), revision: 2,
                                           attempt: String(repeating: "a", count: 32), uploadUrl: "http://127.0.0.1:3000/upload",
                                           sha256: fixture.media.sha256, mediaType: "image/png", byteSize: fixture.media.byteSize,
                                           response: .init(schemaVersion: 1, statusCode: 200, mediaType: "application/json", contentEncoding: nil, body: Data("{}".utf8)))
    do {
      _ = try await runtime.recoverNativeUpload(receipt: receipt, media: media)
      XCTFail("Unsupported receipt must fail")
    } catch let TeraAppError.Failure(report) {
      XCTAssertEqual(report.code, "invalid_recovery_receipt")
    }
    _ = try await runtime.shutdown()
  }

  private static func verified(_ input: TeraRecoveryUploadReceipt) -> TeraRecoveryCompletionReceipt {
    .init(parent: input.parent, attempt: input.attempt, canonicalURL: input.media.remoteURL ?? "",
          sha256: input.media.sha256, mediaType: input.media.mediaType, byteSize: input.media.byteSize, verifiedAtUnixMS: 1_800_000_000_000)
  }
}
