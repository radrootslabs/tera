import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraUploadRenewalFFITests: XCTestCase {
  func testGeneratedRenewalKeepsDeniedReservationsAndEnforcesBudgetAfterReconstruction() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    try await runtime.configureSimulatorRelays(loopbackRelays: ["ws://127.0.0.1:19999"])
    let backend = TeraGeneratedRuntimeBackend(runtime: runtime)
    let request = try await submission(backend: backend, media: fixture.media)
    let file = try fixture.original()
    defer { try? file.close() }
    let handle = try TeraPreparedMediaHandle(media: fixture.media, fileDescriptor: UInt64(file.fileDescriptor))
    let initial = try await backend.prepareSubmission(request: request, media: [handle])
    do { _ = try await backend.prepareSubmissionUpload(input: .init(request: request, expectedRevision: initial.revision, media: handle))
      XCTFail("No signer is installed")
    } catch {}
    var retained = try await backend.submissionStatus(request: request)
    XCTAssertEqual(retained.media[0].authorizations.count, 1)
    for expectedCount in 2 ... 3 {
      // The real runtime clock drives expiry/backoff; the denied native attempt
      // is explicitly reconciled failed for this generated-boundary test.
      try await Task.sleep(for: .milliseconds(1100))
      let prior = try XCTUnwrap(retained.media[0].authorizations.last)
      let renewal = try TeraSubmissionUploadRenewal(priorRevision: XCTUnwrap(prior.revision), priorAttempt: prior.operationID, nativeFailed: true)
      do { _ = try await backend.renewSubmissionUpload(input: .init(request: request, expectedRevision: retained.revision, media: handle), renewal: renewal)
        XCTFail("Renewal must not manufacture a signer")
      } catch {}
      retained = try await backend.submissionStatus(request: request)
      XCTAssertEqual(retained.media[0].authorizations.count, expectedCount)
      XCTAssertEqual(retained.captured, initial.captured)
      XCTAssertEqual(retained.operationID, initial.operationID)
      XCTAssertEqual(retained.intentID, initial.intentID)
    }
    _ = try await runtime.shutdown()
    let reopened = try await fixture.runtime()
    try await reopened.configureSimulatorRelays(loopbackRelays: ["ws://127.0.0.1:19999"])
    let consumer = TeraGeneratedRuntimeBackend(runtime: reopened)
    let restored = try await consumer.submissionStatus(request: request)
    XCTAssertEqual(restored, retained)
    let last = try XCTUnwrap(restored.media[0].authorizations.last)
    do {
      _ = try await consumer.renewSubmissionUpload(input: .init(request: request, expectedRevision: restored.revision, media: handle),
                                                   renewal: .init(priorRevision: XCTUnwrap(last.revision), priorAttempt: last.operationID, nativeFailed: true))
      XCTFail("Reopening cannot reset the authorization budget")
    } catch let failure as TeraRuntimeFailure { XCTAssertEqual(failure.code, "submission_upload_attempts_exhausted") }
    let final = try await consumer.submissionStatus(request: request)
    XCTAssertEqual(final, restored)
    _ = try await reopened.shutdown()
  }

  private func submission(backend: TeraGeneratedRuntimeBackend, media: TeraPreparedMedia) async throws -> TeraSubmissionRequest {
    let scope = TeraComposerScope(authorPublicKey: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798", localNetworkID: "nearby")
    var editing = TeraAddForm.empty(.createPhotoUpdate)
    editing.content = "Renewal fixture"
    editing.media = [media]
    let saved = try await backend.saveComposer(request: .init(scope: scope, id: backend.reserveComposerID(),
                                                              expectedRevision: nil, editSequence: 1, form: TeraComposerForm(editing: editing)))
    return try await TeraSubmissionRequest(commandID: backend.reserveSubmissionID(), scope: scope,
                                           composerID: saved.draft.id, expectedRevision: saved.draft.revision)
  }
}
