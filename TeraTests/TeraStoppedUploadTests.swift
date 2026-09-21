import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

@MainActor
final class TeraStoppedUploadTests: XCTestCase {
  func testStoppedRecoveryReadsExactResponsesWithoutEnqueueRetryOrSettlement() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let status = submission(fixture, revision: 7)
    let missing = try await coordinator.retainedSubmissionUpload(status, media: fixture.media)
    XCTAssertNil(missing)
    let job = fixture.job(revision: 2, operation: String(repeating: "2", count: 32))
    let request = try fixture.request(job: job)
    for state: RadrootsBackgroundTransferState in [.queued, .running, .failed, .cancelled, .interrupted, .expired] {
      try await transfer.seed(request: request, state: state)
      let receipt = try await coordinator.retainedSubmissionUpload(status, media: fixture.media)
      XCTAssertNil(receipt)
    }
    for state: RadrootsBackgroundTransferState in [.awaitingVerification, .completed] {
      try await transfer.seed(request: request, state: state)
      let receipt = try await coordinator.retainedSubmissionUpload(status, media: fixture.media)
      XCTAssertEqual(receipt?.identifier, job.transferIdentifier)
      XCTAssertEqual(receipt?.draftID, fixture.draftID)
      XCTAssertEqual(receipt?.expectedRevision, job.draft.revision)
      XCTAssertNotEqual(receipt?.expectedRevision, status.revision)
      XCTAssertEqual(receipt?.body, Data("{}".utf8))
    }
    let counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 0)
    XCTAssertEqual(counts.retry, 0)
    XCTAssertEqual(counts.cancel, 0)
    XCTAssertEqual(counts.acceptedSettlement, 0)
  }

  func testStoppedRecoveryRejectsWrongDestinationFutureRevisionAndAmbiguousAttempts() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let status = submission(fixture)
    let job = fixture.job(revision: 2, operation: String(repeating: "2", count: 32))
    for scenario in 0 ..< 3 {
      let transfer = BackgroundTransferHarness()
      let coordinator = fixture.coordinator(transfer: transfer)
      let first = scenario == 1 ? fixture.job(revision: 9, operation: String(repeating: "3", count: 32)) : job
      try await transfer.seed(request: fixture.request(job: first, remoteURL: scenario == 0 ? "http://127.0.0.1:3001/upload" : nil), state: .awaitingVerification)
      if scenario == 2 {
        try await transfer.seed(request: fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "4", count: 32))), state: .awaitingVerification)
      }
      do {
        _ = try await coordinator.retainedSubmissionUpload(status, media: fixture.media)
        XCTFail("Mismatched or ambiguous native evidence cannot be consumed")
      } catch {}
      let counts = await transfer.counts
      XCTAssertEqual(counts.enqueue + counts.retry + counts.cancel + counts.acceptedSettlement, 0)
    }
  }

  func testStopCancelsOwnedWaiterBeforeEnqueueAndRetainsAlreadyEnqueuedResponse() async throws {
    for boundary in [BackgroundTransferPause.discovery, .snapshot] {
      let fixture = try BackgroundUploadFixture()
      defer { fixture.remove() }
      let transfer = BackgroundTransferHarness(pause: boundary)
      let coordinator = fixture.coordinator(transfer: transfer)
      let stop = TeraSubmissionStopControl()
      let job = fixture.job(revision: 2, operation: String(repeating: "2", count: 32))
      let task = Task { try await stop.upload(using: coordinator, transfer: job.transfer, source: fixture.media) }
      for _ in 0 ..< 100 {
        if await transfer.isPaused {
          break
        }
        try await Task.sleep(for: .milliseconds(10))
      }
      let paused = await transfer.isPaused
      XCTAssertTrue(paused)
      stop.request()
      await transfer.releasePause()
      do { _ = try await task.value; XCTFail("Stopped waiter cannot report a new result") } catch {}
      let counts = await transfer.counts
      XCTAssertEqual(counts.enqueue, boundary == .discovery ? 0 : 1)
      XCTAssertEqual(counts.retry + counts.cancel + counts.acceptedSettlement, 0)
      if boundary == .snapshot {
        let receipt = try await coordinator.retainedSubmissionUpload(submission(fixture), media: fixture.media)
        XCTAssertEqual(receipt?.identifier, job.transferIdentifier)
      }
    }
  }

  private func submission(_ fixture: BackgroundUploadFixture, revision: UInt64 = 2) -> TeraSubmissionStatus {
    let draft = fixture.draft(revision: revision, stage: .uploading)
    let scope = TeraComposerScope(authorPublicKey: String(repeating: "a", count: 64), localNetworkID: "nearby")
    let request = TeraSubmissionRequest(commandID: String(repeating: "3", count: 32), scope: scope,
                                        composerID: String(repeating: "4", count: 32), expectedRevision: 1)
    return TeraSubmissionStatus(request: request, intentID: fixture.draftID, operationID: String(repeating: "5", count: 32),
                                revision: draft.revision, captured: TeraComposerDraft(scope: scope, id: request.composerID, revision: 1, editSequence: 1,
                                                                                      form: TeraComposerForm(editing: draft.form!)), state: .cancelled,
                                committedAtUnixMilliseconds: draft.createdAtUnixMilliseconds, updatedAtUnixMilliseconds: draft.updatedAtUnixMilliseconds,
                                media: [TeraSubmissionMedia(opaqueReference: fixture.media.opaqueReference, progress: draft.media[0])],
                                settlement: TeraOperationSettlement(artifacts: 1, signed: 0, admitted: 0, pending: 0, retryable: 0,
                                                                    indeterminate: 0, failedTerminal: 0, cancelled: 1, deliveryPlans: 1, deliverySatisfied: 0, deliveryPending: 0,
                                                                    deliveryRetryable: 0, deliveryExhausted: 0, deliveryFailedTerminal: 0, deliveryCancelled: 1),
                                delivery: TeraPublicationEvidence(state: .notIssued, stopRequestedAtUnixMilliseconds: 1_800_000_000_002,
                                                                  schedulingRevision: 2, retainedFacts: 0, recordedAttempts: 0, unresolvedClaims: false),
                                targetDetails: .fixture(), retry: .stopped)
  }
}
