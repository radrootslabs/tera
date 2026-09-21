import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

@MainActor
final class TeraPublicationRetryTests: XCTestCase {
  func testTypedDecisionsRetainSpecificActionAndWaitingReasons() throws {
    let cases: [(FfiPublicationActionReason, TeraPublicationActionReason)] = [
      (.deadlineExceeded, .deadlineExceeded), (.attemptLimit, .attemptLimit),
      (.authenticationRequired, .authenticationRequired), (.quotaExceeded, .quotaExceeded),
      (.invalidPayload, .invalidPayload), (.deliveryRefused, .deliveryRefused),
      (.coordinateChanged, .coordinateChanged),
    ]
    for (wire, expected) in cases {
      let decoded = try TeraPublicationRetry.decode(.needsAction(reason: wire))
      XCTAssertEqual(decoded, .needsAction(expected))
      XCTAssertFalse(decoded.mayStart)
      XCTAssertTrue(decoded.explanation?.contains("retain") == true)
    }
    XCTAssertTrue(try TeraPublicationRetry.decode(.ready).mayStart)
    XCTAssertFalse(try TeraPublicationRetry.decode(.complete).mayStart)
    XCTAssertFalse(try TeraPublicationRetry.decode(.stopped).mayStart)
    XCTAssertEqual(try TeraPublicationRetry.decode(.deferredUntil(unixMs: 100)), .deferredUntil(100))
    XCTAssertEqual(try TeraPublicationRetry.decode(.inFlightUntil(unixMs: 100)), .inFlightUntil(100))
    // A native clock cannot substitute for a fresh Rust scheduling decision.
    XCTAssertFalse(TeraPublicationRetry.deferredUntil(1).mayStart)
    XCTAssertFalse(TeraPublicationRetry.inFlightUntil(1).mayStart)
  }

  func testMalformedWaitingBoundsFailClosed() {
    for value in [UInt64(0), UInt64.max] {
      XCTAssertThrowsError(try TeraPublicationRetry.decode(.deferredUntil(unixMs: value)))
      XCTAssertThrowsError(try TeraPublicationRetry.decode(.inFlightUntil(unixMs: value)))
    }
  }

  func testNeedsActionAndWaitDoNotAdvanceAStillQueuedSubmission() async throws {
    let backend = AddBackend(advanceOffline: true)
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Saved original")
    await store.submit()
    let original = try XCTUnwrap(store.submissions.status)
    XCTAssertEqual(original.state, .queued)
    let before = await backend.submissionBackend.advanceCount
    let decisions: [TeraPublicationRetry] = [
      .deferredUntil(1), .inFlightUntil(1), .needsAction(.deadlineExceeded), .needsAction(.attemptLimit),
      .needsAction(.authenticationRequired), .needsAction(.quotaExceeded),
      .needsAction(.invalidPayload), .needsAction(.deliveryRefused),
      .needsAction(.coordinateChanged),
    ]
    for decision in decisions {
      let status = TeraSubmissionStatus(
        request: original.request, intentID: original.intentID, operationID: original.operationID,
        revision: original.revision, captured: original.captured, state: original.state,
        committedAtUnixMilliseconds: original.committedAtUnixMilliseconds,
        updatedAtUnixMilliseconds: original.updatedAtUnixMilliseconds,
        media: original.media, settlement: original.settlement, delivery: original.delivery,
        targetDetails: original.targetDetails, retry: decision
      )
      XCTAssertFalse(status.canOfferContinuation)
      let effects = TeraSubmissionEffects(client: client, media: nil, ensure: {},
                                          accept: { _ in XCTFail("Blocked work cannot advance") })
      try await effects.advance(status)
      XCTAssertEqual(status.captured, original.captured)
      XCTAssertEqual(status.targetDetails, original.targetDetails)
    }
    let after = await backend.submissionBackend.advanceCount
    XCTAssertEqual(after, before)
    store.stop()
    _ = try await client.stop()
  }
}
