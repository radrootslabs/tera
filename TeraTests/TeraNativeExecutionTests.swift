import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraNativeExecutionTests: XCTestCase {
  func testLostEnqueueReplyRejoinsExactReceiptWithoutAnotherEffect() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = NativeExecutionTransfer()
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    let receipt = try await fixture.coordinator(transfer: transfer).uploadInBackground(job: job, media: fixture.media)
    XCTAssertEqual(receipt.identifier, job.transferIdentifier)
    XCTAssertEqual(receipt.expectedRevision, 2)
    let later = fixture.job(revision: 3, operation: String(repeating: "b", count: 32))
    let replay = try await fixture.coordinator(transfer: transfer).uploadInBackground(job: later, media: fixture.media)
    XCTAssertEqual(replay, receipt)
    let counts = await transfer.base.counts
    XCTAssertEqual(counts.enqueue, 1)
    XCTAssertEqual(counts.retry, 0)
    XCTAssertEqual(counts.acceptedSettlement, 0)
  }

  func testLostRetryReplyDoesNotScheduleAnotherRetry() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = NativeExecutionTransfer()
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    try await transfer.base.seed(request: fixture.request(job: job), state: .interrupted)
    let receipt = try await fixture.coordinator(transfer: transfer).uploadInBackground(job: job, media: fixture.media)
    XCTAssertEqual(receipt.identifier, job.transferIdentifier)
    let counts = await transfer.base.counts
    XCTAssertEqual(counts.retry, 1)
    XCTAssertEqual(counts.enqueue, 0)
  }

  func testUnknownInventoryDoesNotEnqueueRetryOrSelectForegroundFallback() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = NativeExecutionTransfer(unknownInventory: true)
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    try await transfer.base.seed(request: fixture.request(job: job), state: .running)
    let before = try await transfer.base.snapshots()
    let coordinator = fixture.coordinator(transfer: transfer)
    do {
      _ = try await coordinator.uploadInBackground(job: job, media: fixture.media)
      XCTFail("Unknown inventory cannot admit an upload")
    } catch let error as RadrootsBackgroundTransferError { XCTAssertEqual(error, .transferFailure) }
    do {
      _ = try await coordinator.prefersSharedForegroundUpload(ownerID: fixture.draftID)
      XCTFail("Unknown native inventory cannot authorize foreground fallback")
    } catch let error as RadrootsBackgroundTransferError { XCTAssertEqual(error, .transferFailure) }
    let after = try await transfer.base.snapshots()
    XCTAssertEqual(after, before)
    let counts = await transfer.base.counts
    XCTAssertEqual(counts.enqueue, 0)
    XCTAssertEqual(counts.retry, 0)
    XCTAssertEqual(counts.cancel, 0)
  }

  func testBoundedObservationPreservesRunningTransferAndLaterOriginalReceipt() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    try await transfer.seed(request: request, state: .running)
    let before = try await transfer.snapshots()
    do {
      _ = try await TeraBackgroundUploadWaiter.receipt(transfer: transfer, for: request.identifier,
                                                       draftID: fixture.draftID, expectedRevision: 3, request: request, waitNanoseconds: 1_000_000)
      XCTFail("A still-running transfer must remain unknown after the observation budget")
    } catch let failure as TeraRuntimeFailure { XCTAssertEqual(failure.code, "ios.add.background_upload_unknown") }
    let after = try await transfer.snapshots()
    XCTAssertEqual(after, before)
    try await transfer.setState(.awaitingVerification)
    let receipt = try await TeraBackgroundUploadWaiter.receipt(transfer: transfer, for: request.identifier,
                                                               draftID: fixture.draftID, expectedRevision: 3, request: request)
    XCTAssertEqual(receipt.expectedRevision, 2)
    let counts = await transfer.counts
    XCTAssertEqual(counts.cancel, 0)
    XCTAssertEqual(counts.retry, 0)
    XCTAssertEqual(counts.enqueue, 0)
    XCTAssertEqual(counts.acceptedSettlement, 0)
  }

  func testObservationRejectsExecutionReplacementWithoutMutatingNativeState() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    for carryAdmission in [false, true] {
      let transfer = NativeExecutionTransfer(replaceExecution: true)
      try await transfer.base.seed(request: request, state: .running)
      let baseline = carryAdmission ? try await transfer.snapshot(for: request.identifier) : nil
      do {
        _ = try await TeraBackgroundUploadWaiter.receipt(transfer: transfer, for: request.identifier,
                                                         draftID: fixture.draftID, expectedRevision: 2, request: request, baseline: baseline)
        XCTFail("A replacement execution cannot supply this observation's receipt")
      } catch let failure as TeraRuntimeFailure { XCTAssertEqual(failure.code, "ios.add.background_upload_mismatch") }
      let counts = await transfer.base.counts
      XCTAssertEqual(counts.cancel, 0)
      XCTAssertEqual(counts.retry, 0)
      XCTAssertEqual(counts.acceptedSettlement, 0)
    }
  }

  func testRealNativeAdmissionRefusesDuplicateAfterLostReplyAndRelaunch() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let platform = LostNativeAdmission()
    let store = fixture.nativeStore
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    for _ in 0 ..< 2 {
      let native = RadrootsAppleBackgroundTransfer(store: store, adapters: platform.adapters)
      do {
        _ = try await fixture.coordinator(transfer: native).uploadInBackground(job: job, media: fixture.media)
        XCTFail("An active task with a lost response is unknown")
      } catch let failure as TeraRuntimeFailure { XCTAssertEqual(failure.code, "ios.add.background_upload_unknown") }
    }
    let starts = await platform.starts
    XCTAssertEqual(starts, 1)
    let snapshots = try await store.loadSnapshots()
    let retained = try XCTUnwrap(snapshots.first)
    XCTAssertNotNil(retained.executionID)
    XCTAssertTrue(retained.possibleRemoteOrphan)
    let late = try RadrootsBackgroundTransferSnapshot(request: retained.request, state: .awaitingVerification,
                                                      response: RadrootsBackgroundTransferResponse(statusCode: 200, mediaType: "application/json", body: Data("{}".utf8)),
                                                      executionID: retained.executionID)
    try await store.saveSnapshot(late)
    let native = RadrootsAppleBackgroundTransfer(store: fixture.nativeStore, adapters: platform.adapters)
    let receipt = try await fixture.coordinator(transfer: native).uploadInBackground(job: job, media: fixture.media)
    XCTAssertEqual(receipt.identifier, job.transferIdentifier)
    let after = await platform.starts
    XCTAssertEqual(after, 1)
  }
}
