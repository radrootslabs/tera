import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraPublicationStageTests: XCTestCase {
  func testEveryDurablePhaseHasScopedStageAndEligibleActions() async throws {
    let backend = AddBackend(advanceOffline: true)
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Saved original")
    await store.submit()
    let original = try XCTUnwrap(store.submissions.status)
    let expected: [(TeraOutboxState, String)] = [
      (.draft, "Saved on this device."), (.mediaPreparing, "Preparing photo."),
      (.mediaUploading, "Photo upload awaiting verification."),
      (.readyToSign, "Awaiting signing."), (.signing, "Awaiting signing."),
      (.signed, "Signed; local admission is pending."), (.queued, "Queued for the saved relays."),
      (.delivering, "Sending to the saved relays."),
      (.partiallyDelivered, "Partially delivered; review the saved relay outcomes."),
      (.retryable, "Saved for retry."), (.terminal, "Publication needs attention."),
      (.cancelled, "Local work stopped. Recorded remote effects are retained."),
      (.complete, "Delivery completed for the saved relay policy."),
    ]
    XCTAssertEqual(Set(expected.map(\.0)), Set(TeraOutboxState.allCases))
    for (state, label) in expected {
      let value = replacing(original, state: state)
      XCTAssertEqual(value.summary, label)
      XCTAssertEqual(state.summary(settlement: nil), label)
      XCTAssertEqual(value.canOfferContinuation, ![.complete, .cancelled, .terminal].contains(state))
      XCTAssertEqual(value.canOfferStop, state.canCancel)
    }
    store.stop()
    _ = try await client.stop()
  }

  func testLegacySatisfiedPlanDoesNotHideMixedOrStoppedEffects() {
    let settlement = TeraOperationSettlement(
      artifacts: 7, signed: 4, admitted: 3, pending: 1, retryable: 1,
      indeterminate: 1, failedTerminal: 1, cancelled: 1,
      deliveryPlans: 6, deliverySatisfied: 1, deliveryPending: 1,
      deliveryRetryable: 1, deliveryExhausted: 1, deliveryFailedTerminal: 1, deliveryCancelled: 1
    )
    let summary = TeraOutboxState.cancelled.summary(settlement: settlement)
    for retained in ["Local work stopped", "1 of 6", "unknown", "pending", "saved for retry",
                     "limit was reached", "failure", "effects are retained", "4 signed", "3 admitted locally"]
    {
      XCTAssertTrue(summary.contains(retained), retained)
    }
    XCTAssertFalse(summary.contains("Delivery completed"))
    XCTAssertFalse(summary.contains("Published"))
    let draft = TeraDraftStatus(
      id: "draft", revision: 1, authorPublicKey: "author", kind: .add, commandType: .createUpdate,
      form: nil, state: .cancelled, cardID: "card", operationID: "operation",
      createdAtUnixMilliseconds: 1, updatedAtUnixMilliseconds: 2,
      media: [], settlement: settlement, isRevision: false
    )
    XCTAssertEqual(draft.honestSummary, summary)
    let legacy = TeraLegacyDraftSummary(
      id: draft.id, revision: 1, kind: .add, commandType: .createUpdate, state: .cancelled,
      hasForm: false, isRevision: false, createdAtUnixMilliseconds: 1, updatedAtUnixMilliseconds: 2,
      mediaCount: 0, verifiedMediaCount: 0, possibleOrphanCount: 0, settlement: settlement
    )
    XCTAssertEqual(legacy.honestSummary, summary)
  }

  func testMainSubmitAndAccessibleStatusHonorEverySavedHoldWithoutEffects() async throws {
    let backend = AddBackend(advanceOffline: true)
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Saved original")
    await store.submit()
    let original = try XCTUnwrap(store.submissions.status)
    let before = await backend.submissionBackend.advanceCount
    let decisions: [(TeraPublicationRetry, String)] = [
      (.deferredUntil(1), "Publication waiting"), (.inFlightUntil(1), "Publication waiting"),
      (.needsAction(.deadlineExceeded), "Publication needs attention"),
      (.needsAction(.attemptLimit), "Publication needs attention"),
      (.needsAction(.coordinateChanged), "Publication needs attention"),
      (.needsAction(.authenticationRequired), "Publication needs attention"),
      (.needsAction(.quotaExceeded), "Publication needs attention"),
      (.needsAction(.invalidPayload), "Publication needs attention"),
      (.needsAction(.deliveryRefused), "Publication needs attention"),
    ]
    for (retry, label) in decisions {
      let value = replacing(original, retry: retry)
      await backend.submissionBackend.installPresentationFixture(value)
      await store.submissions.refreshSelected()
      XCTAssertFalse(store.canSubmit)
      XCTAssertFalse(store.submissions.canContinue)
      XCTAssertEqual(store.submitLabel, label)
      XCTAssertTrue(try store.submitAccessibilityValue.contains(XCTUnwrap(retry.explanation)))
      await store.submit()
      await store.submissions.continueSelected()
      XCTAssertEqual(store.submissions.status?.request, original.request)
      XCTAssertEqual(store.submissions.status?.captured, original.captured)
    }
    let after = await backend.submissionBackend.advanceCount
    XCTAssertEqual(after, before)
    await backend.submissionBackend.installPresentationFixture(original)
    await store.submissions.refreshSelected()
    XCTAssertTrue(store.canSubmit)
    XCTAssertEqual(store.submitLabel, "Continue original submission")
    store.stop()
    _ = try await client.stop()
  }

  func testAcceptedCompletionAndStoppedEffectsRemainAccessibleWithoutRetry() async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Saved original")
    await store.submit()
    XCTAssertEqual(store.submissions.status?.state, .complete)
    XCTAssertFalse(store.canSubmit)
    XCTAssertEqual(store.submitLabel, "Delivery policy complete")
    XCTAssertTrue(store.submitAccessibilityValue.contains("saved relay policy"))
    XCTAssertFalse(try XCTUnwrap(store.submissions.status).canOfferStop)
    // A previously requested stop can arrive after acceptance; preserve both.
    await store.submissions.requestStop()
    XCTAssertFalse(store.canSubmit)
    XCTAssertEqual(store.submitLabel, "Publication stopped")
    XCTAssertTrue(store.submitAccessibilityValue.contains("Stopped"))
    XCTAssertTrue(store.submitAccessibilityValue.contains("accepted"))
    XCTAssertTrue(try XCTUnwrap(store.submissions.status).targetDetails.targets[0].accepted)
    await store.submissions.refreshSelected()
    XCTAssertTrue(store.submitAccessibilityValue.contains("accepted"))
    store.stop()
    _ = try await client.stop()
  }

  func testPartialUnknownAndStoppedFactsTakePrecedenceOverNominalStage() async throws {
    let backend = AddBackend(advanceOffline: true)
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Saved original")
    await store.submit()
    let original = try XCTUnwrap(store.submissions.status)
    for delivery in [TeraPublicationDeliveryState.unknown, .partiallyAccepted, .accepted] {
      let evidence = TeraPublicationEvidence(
        state: delivery, stopRequestedAtUnixMilliseconds: nil, schedulingRevision: 2,
        retainedFacts: 1, recordedAttempts: 1, unresolvedClaims: true
      )
      let current = replacing(original, state: .complete, delivery: evidence)
      XCTAssertTrue(current.canOfferStop)
      XCTAssertFalse(current.canOfferContinuation)
      if delivery != .accepted {
        XCTAssertFalse(current.summary.contains("completed"))
      }
      let stopped = TeraPublicationEvidence(
        state: delivery, stopRequestedAtUnixMilliseconds: 10, schedulingRevision: 3,
        retainedFacts: 1, recordedAttempts: 1, unresolvedClaims: true
      )
      let value = replacing(original, state: .cancelled, delivery: stopped)
      XCTAssertEqual(value.summary, stopped.stoppedSummary)
      XCTAssertFalse(value.canOfferContinuation)
      XCTAssertFalse(value.canOfferStop)
    }
    store.stop()
    _ = try await client.stop()
  }

  private func replacing(_ original: TeraSubmissionStatus, state: TeraOutboxState = .queued,
                         retry: TeraPublicationRetry = .ready, delivery: TeraPublicationEvidence? = nil) -> TeraSubmissionStatus
  {
    TeraSubmissionStatus(
      request: original.request, intentID: original.intentID, operationID: original.operationID,
      revision: original.revision, captured: original.captured, state: state,
      committedAtUnixMilliseconds: original.committedAtUnixMilliseconds,
      updatedAtUnixMilliseconds: original.updatedAtUnixMilliseconds,
      media: original.media, settlement: original.settlement, delivery: delivery ?? original.delivery,
      targetDetails: original.targetDetails, retry: retry
    )
  }
}
