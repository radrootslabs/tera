import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraUploadRenewalExecutionTests: XCTestCase {
  func testFreshNativeIdentifierEnqueuesWhileOldAdmissionRemainsHeld() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let prior = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    let store = fixture.nativeStore
    let original = try RadrootsBackgroundTransferSnapshot(request: prior, state: .expired)
    try await store.saveSnapshot(original)
    let inert = RadrootsAppleBackgroundTransferAdapters(enqueue: { _, _ in XCTFail("Old authority was replayed") },
                                                        cancel: { _ in XCTFail("Unexpected cancellation") }, activeTransferIdentifiers: { [] }, handleBackgroundEvents: { _, done in done() })
    let other = RadrootsAppleBackgroundTransfer(store: fixture.nativeStore, adapters: inert)
    let backend = RenewalExecutionBackend(fixture: fixture)
    let adapters = RadrootsAppleBackgroundTransferAdapters(enqueue: { request, executionID in
      XCTAssertNotEqual(request.identifier, prior.identifier)
      XCTAssertEqual(request.identifier.rawValue, fixture.job(revision: 3, operation: String(repeating: "b", count: 32)).transferIdentifier)
      XCTAssertEqual(request.headers["Authorization"], "Nostr test-authorization")
      do { _ = try await other.retry(prior); XCTFail("Old admission released before fresh enqueue") } catch {}
      let queued = try await store.loadSnapshots().first { $0.identifier == request.identifier }
      let pending = try RadrootsBackgroundTransferSnapshot(request: XCTUnwrap(queued).request, state: .awaitingVerification,
                                                           response: RadrootsBackgroundTransferResponse(statusCode: 200, mediaType: "application/json", body: Data("{}".utf8)), executionID: executionID)
      try await store.saveSnapshot(pending)
    }, cancel: { _ in XCTFail("Unexpected cancellation") }, activeTransferIdentifiers: { [] }, handleBackgroundEvents: { _, done in done() })
    let transfer = RadrootsAppleBackgroundTransfer(store: store, adapters: adapters)
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "36")
    let client = TeraRuntimeClient(factory: { _ in .init(backend: backend, snapshot: TeraScopeFixtures.snapshot()) })
    _ = try await client.start(configuration: configuration)
    let initial = await backend.status(revision: 2)
    let result = try await fixture.coordinator(transfer: transfer).renewSubmissionUpload(initial, media: fixture.media, client: client)
    XCTAssertEqual(result.intentID, initial.intentID)
    XCTAssertEqual(result.operationID, initial.operationID)
    XCTAssertEqual(result.captured, initial.captured)
    let counts = await backend.counts
    XCTAssertEqual(counts, [1, 1])
    let unchanged = try await store.loadSnapshots().first { $0.identifier == prior.identifier }
    XCTAssertEqual(unchanged, original)
    let snapshots = try await store.loadSnapshots()
    XCTAssertEqual(snapshots.filter { $0.state == .completed }.count, 1)
    _ = try await client.stop()
  }
}

/// Tests native scheduling only; Rust authority and canonical byte verification
/// are exercised independently through the actual generated runtime.
private actor RenewalExecutionBackend: TeraRuntimeBackend {
  let fixture: BackgroundUploadFixture
  var revision: UInt64 = 2
  var counts = [0, 0]
  init(fixture: BackgroundUploadFixture) {
    self.fixture = fixture
  }

  func status(revision: UInt64) -> TeraSubmissionStatus {
    let draft = fixture.draft(revision: revision, stage: revision == 4 ? .verified : .uploading)
    let scope = TeraComposerScope(authorPublicKey: String(repeating: "a", count: 64), localNetworkID: "nearby")
    let request = TeraSubmissionRequest(commandID: String(repeating: "3", count: 32), scope: scope,
                                        composerID: String(repeating: "4", count: 32), expectedRevision: 1)
    let old = TeraUploadAttemptIdentity(operationID: String(repeating: "a", count: 32), revision: 2, expirationUnixSeconds: 100)
    let next = TeraUploadAttemptIdentity(operationID: String(repeating: "b", count: 32), revision: 3, expirationUnixSeconds: 200)
    return TeraSubmissionStatus(request: request, intentID: fixture.draftID, operationID: String(repeating: "5", count: 32),
                                revision: revision, captured: TeraComposerDraft(scope: scope, id: request.composerID, revision: 1, editSequence: 1,
                                                                                form: TeraComposerForm(editing: draft.form!)), state: revision == 4 ? .readyToSign : .mediaUploading,
                                committedAtUnixMilliseconds: 1, updatedAtUnixMilliseconds: 2,
                                media: [.init(opaqueReference: fixture.media.opaqueReference, progress: draft.media[0], authorizations: revision == 2 ? [old] : [old, next])],
                                settlement: TeraOperationSettlement(artifacts: 1, signed: 0, admitted: 0, pending: 1, retryable: 0,
                                                                    indeterminate: 0, failedTerminal: 0, cancelled: 0, deliveryPlans: 1, deliverySatisfied: 0, deliveryPending: 1,
                                                                    deliveryRetryable: 0, deliveryExhausted: 0, deliveryFailedTerminal: 0, deliveryCancelled: 0),
                                delivery: TeraPublicationEvidence(state: .notIssued, stopRequestedAtUnixMilliseconds: nil,
                                                                  schedulingRevision: revision, retainedFacts: 0, recordedAttempts: 0, unresolvedClaims: false),
                                targetDetails: .fixture())
  }

  func renewSubmissionUpload(input: TeraSubmissionMediaRequest, renewal: TeraSubmissionUploadRenewal) throws -> TeraSubmissionUploadJob {
    XCTAssertEqual(input.expectedRevision, 2)
    XCTAssertEqual(renewal, .init(priorRevision: 2, priorAttempt: String(repeating: "a", count: 32), nativeFailed: true))
    counts[0] += 1; revision = 3
    return .init(submission: status(revision: revision), transfer: fixture.job(revision: revision, operation: String(repeating: "b", count: 32)).transfer)
  }

  func submissionStatus(request _: TeraSubmissionRequest) -> TeraSubmissionStatus {
    status(revision: revision)
  }

  func completeSubmissionUpload(input: TeraSubmissionMediaRequest, response: TeraAddBackgroundUploadReceipt) -> TeraSubmissionStatus {
    XCTAssertEqual(input.expectedRevision, 3)
    XCTAssertEqual(response.expectedRevision, 3)
    XCTAssertEqual(response.identifier, fixture.job(revision: 3, operation: String(repeating: "b", count: 32)).transferIdentifier)
    counts[1] += 1; revision = 4
    return status(revision: revision)
  }

  func snapshot() -> TeraRuntimeSnapshot {
    TeraScopeFixtures.snapshot()
  }

  func todayPage(request _: TeraTodayPageRequest) throws -> TeraTodayPage {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func refreshToday(context _: TeraLocalNetwork, nowUnixSeconds _: UInt64, update _: TeraTodayProjectionUpdate, backfillCursor _: String?) throws -> TeraTodaySyncReceipt {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func subscribe(bufferCapacity _: Int, receive _: @escaping @Sendable (TeraRuntimeChange) async -> Void) -> any TeraRuntimeSubscriptionToken {
    ResourceTestToken()
  }

  func shutdown() -> TeraRuntimeShutdownReceipt {
    .init(state: "closed", alreadyClosed: false)
  }
}
