import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraSubmissionReservationTests: XCTestCase {
  private let scope = TeraComposerScope(
    authorPublicKey: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798", localNetworkID: "nearby"
  )

  func testProductionReservationSurvivesSessionChangeAndNewerEditingWithoutSigning() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let signer = ComposerForbiddenSigner()
    let client = TeraRuntimeClient.production()
    let configuration = configuration(fixture, signer: signer)
    _ = try await client.start(configuration: configuration)
    let firstLifecycle = await client.lifecycle()
    let id = try await client.reserveComposerID()
    var form = TeraComposerForm(commandType: .createEvent)
    form.content = "PRIVATE unfinished café"
    form.eventStartDate = "2026-09-"
    let saved = try await client.saveComposer(request: TeraComposerSaveRequest(
      scope: scope, id: id, expectedRevision: nil, editSequence: 1, form: form
    ))
    let commandID = try await client.reserveSubmissionID()
    let request = TeraSubmissionRequest(commandID: commandID, scope: scope, composerID: id, expectedRevision: 1)
    let first = try await client.reserveSubmission(request: request)
    XCTAssertEqual(first.captured, saved.draft)
    XCTAssertFalse(first.replayed)
    form.content = "later editing"
    let latest = try await client.saveComposer(request: TeraComposerSaveRequest(
      scope: scope, id: id, expectedRevision: 1, editSequence: 2, form: form
    ))
    _ = try await client.stop()
    _ = try await client.start(configuration: configuration)
    let secondLifecycle = await client.lifecycle()
    XCTAssertNotEqual(firstLifecycle, secondLifecycle)
    let replay = try await client.reserveSubmission(request: request)
    XCTAssertTrue(replay.replayed)
    XCTAssertEqual(replay.captured, saved.draft)
    XCTAssertEqual(replay.reservationID, first.reservationID)
    XCTAssertEqual(replay.reservedAtUnixMilliseconds, first.reservedAtUnixMilliseconds)
    let current = try await client.loadComposer(scope: scope, id: id)
    XCTAssertEqual(current, latest.draft)
    let operations = try await client.draftHeads()
    XCTAssertTrue(operations.isEmpty)
    _ = try await client.stop()
    let calls = await signer.requests
    XCTAssertEqual(calls, 0)
  }

  func testGeneratedReservationsReuseDuplicatesAndRejectChangedSources() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let id = try composerReserveId().id
    let form = TeraComposerForm(commandType: .createUpdate)
    let saved = try await runtime.composerSave(request: TeraComposerSaveRequest(
      scope: scope, id: id, expectedRevision: nil, editSequence: 1, form: form
    ).generatedValue)
    let request = try FfiSubmissionReservationRequest(schemaVersion: 1, commandId: submissionReserveId().id,
                                                      scope: scope.generatedValue, composerId: id, expectedRevision: 1)
    async let left = runtime.submissionReserve(request: request)
    async let right = runtime.submissionReserve(request: request)
    let (first, replay) = try await (left, right)
    XCTAssertEqual(first.reservationId, replay.reservationId)
    XCTAssertNotEqual(first.replayed, replay.replayed)
    XCTAssertEqual(first.captured, saved.draft)
    var changed = request
    changed.expectedRevision = 2
    try await assertFailure(runtime, request: changed, code: "idempotency_conflict")
    changed = request
    changed.scope.localNetworkId = "elsewhere"
    try await assertFailure(runtime, request: changed, code: "idempotency_conflict")
    changed = request
    changed.commandId = try submissionReserveId().id
    let intentional = try await runtime.submissionReserve(request: changed)
    XCTAssertEqual(intentional.captured, first.captured)
    XCTAssertNotEqual(intentional.reservationId, first.reservationId)
    _ = try await runtime.shutdown()
  }

  func testGeneratedVersionAndIdentityFailuresAndUnconfirmedReceiptsFailClosed() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let id = try composerReserveId().id
    let saved = try await runtime.composerSave(request: TeraComposerSaveRequest(
      scope: scope, id: id, expectedRevision: nil, editSequence: 1, form: TeraComposerForm(commandType: .createUpdate)
    ).generatedValue)
    let request = try FfiSubmissionReservationRequest(schemaVersion: 1, commandId: submissionReserveId().id,
                                                      scope: scope.generatedValue, composerId: id, expectedRevision: 1)
    var invalid = request
    invalid.schemaVersion = 2
    try await assertFailure(runtime, request: invalid, code: "submission_schema_unsupported")
    invalid = request
    invalid.scope.schemaVersion = 2
    try await assertFailure(runtime, request: invalid, code: "composer_schema_unsupported")
    invalid = request
    invalid.commandId = String(repeating: "0", count: 32)
    try await assertFailure(runtime, request: invalid, code: "submission_command_id_invalid")
    let receipt = try await runtime.submissionReserve(request: request)
    let input = TeraSubmissionRequest(commandID: request.commandId, scope: scope, composerID: id, expectedRevision: 1)
    XCTAssertEqual(try TeraGeneratedSubmission.translate(receipt, request: input).captured, try saved.draft.composerAppValue)
    var wrong = receipt
    wrong.captured.revision = 2
    XCTAssertThrowsError(try TeraGeneratedSubmission.translate(wrong, request: input))
    wrong = receipt
    wrong.schemaVersion = 2
    XCTAssertThrowsError(try TeraGeneratedSubmission.translate(wrong, request: input))
    wrong = receipt
    wrong.reservationId = String(repeating: "0", count: 32)
    XCTAssertThrowsError(try TeraGeneratedSubmission.translate(wrong, request: input))
    _ = try await runtime.shutdown()
  }

  private func assertFailure(_ runtime: TeraRuntime, request: FfiSubmissionReservationRequest, code: String) async throws {
    do {
      _ = try await runtime.submissionReserve(request: request)
      XCTFail("Invalid reservation must not be acknowledged.")
    } catch let TeraAppError.Failure(report) {
      XCTAssertEqual(report.code, code)
      XCTAssertFalse(report.retryable)
    }
  }

  private func configuration(_ fixture: MediaOwnershipFixture, signer: ComposerForbiddenSigner) -> TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: fixture.root.path, publicKeyHex: scope.authorPublicKey,
      sourceGenerationHex: String(repeating: "04", count: 32), sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
      protectedData: .available, networkProfile: .publicNetwork, writableRelays: ["wss://relay.example"], blossom: nil,
      app: TeraRuntimeAppMetadata(bundleIdentifier: "test.submission", version: "1", buildNumber: "1", buildSHA: nil),
      signerGeneration: "submission-test", signer: signer, adoptBootstrapSettings: false
    )
  }
}
