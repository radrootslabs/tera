import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraSubmissionOperationFFITests: XCTestCase {
  private let scope = TeraComposerScope(
    authorPublicKey: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798", localNetworkID: "nearby"
  )

  @MainActor
  func testNewerEditingSavesAcrossBlockedAndFailedMediaPreparationWithoutChangingReservedSource() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let client = TeraRuntimeClient.production()
    let configuration = configuration(fixture, signer: ComposerForbiddenSigner())
    let snapshot = try await client.start(configuration: configuration)
    let pause = ResourceTestPause()
    let media = AddMediaHarness(openPause: pause, failOpen: true)
    let store = TeraAddStore(runtimeClient: client, media: media)
    store.configure(snapshot: snapshot)
    await store.start()
    store.selectType(.createPhotoUpdate)
    store.updateForm(\.content, "original reserved photo")
    store.updateForm(\.media, [fixture.media])
    let submit = Task { await store.submit() }
    await pause.entered.wait()
    let request = try XCTUnwrap(store.submissions.request)
    let reserved = try await client.reserveSubmission(request: request)
    XCTAssertEqual(reserved.captured.form.content, "original reserved photo")
    store.updateForm(\.content, "editing during blocked preparation")
    XCTAssertTrue(store.canSave)
    await store.save()
    XCTAssertEqual(store.savedComposer?.form.content, "editing during blocked preparation")
    await pause.resume.open()
    await submit.value
    XCTAssertEqual(store.submissions.request, request)
    XCTAssertNil(store.submissions.status)
    store.updateForm(\.content, "editing after preparation failed")
    XCTAssertTrue(store.canSave)
    await store.save()
    let editing = try XCTUnwrap(store.savedComposer)
    XCTAssertNotEqual(editing.id, request.composerID)
    XCTAssertEqual(editing.form.content, "editing after preparation failed")
    let original = try await client.loadComposer(scope: request.scope, id: request.composerID)
    XCTAssertEqual(original, reserved.captured)
    store.stop()
    _ = try await client.stop()
    _ = try await client.start(configuration: configuration)
    let restored = try await client.loadComposer(scope: editing.scope, id: editing.id)
    let replay = try await client.reserveSubmission(request: request)
    XCTAssertEqual(restored, editing)
    XCTAssertEqual(restored.form.content, "editing after preparation failed")
    XCTAssertEqual(replay.captured, reserved.captured)
    XCTAssertTrue(replay.replayed)
    // The original source head still satisfies the production prepare CAS.
    let file = try fixture.original()
    defer { try? file.close() }
    let handle = try TeraPreparedMediaHandle(media: fixture.media, fileDescriptor: UInt64(file.fileDescriptor))
    let committed = try await client.prepareSubmission(request: request, media: [handle])
    XCTAssertEqual(committed.captured, reserved.captured)
    XCTAssertEqual(committed.request, request)
    _ = try await client.stop()
  }

  func testAllFiveProductionCapturesReplayAcrossEditingAndSQLiteReconstruction() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    try runtime.configureSimulatorRelays(loopbackRelays: ["ws://127.0.0.1:19999"])
    let backend = TeraGeneratedRuntimeBackend(runtime: runtime)
    let file = try fixture.original()
    defer { try? file.close() }
    let media = try TeraPreparedMediaHandle(media: fixture.media, fileDescriptor: UInt64(file.fileDescriptor))
    var committed: [TeraSubmissionStatus] = []
    for type in TeraAddCommandType.allCases {
      let form = form(type, media: fixture.media)
      let id = try await backend.reserveComposerID()
      let saved = try await backend.saveComposer(request: TeraComposerSaveRequest(
        scope: scope, id: id, expectedRevision: nil, editSequence: 1, form: form
      ))
      let request = try await TeraSubmissionRequest(commandID: backend.reserveSubmissionID(),
                                                    scope: scope, composerID: id, expectedRevision: saved.draft.revision)
      let handles = type == .createPhotoUpdate ? [media] : []
      async let left = backend.prepareSubmission(request: request, media: handles)
      async let right = backend.prepareSubmission(request: request, media: handles)
      let (first, duplicate) = try await (left, right)
      XCTAssertEqual(first, duplicate)
      XCTAssertEqual(first.captured, saved.draft)
      XCTAssertEqual(first.state, type == .createPhotoUpdate ? .mediaPreparing : .readyToSign)
      XCTAssertEqual(first.settlement.signed, 0)
      var editing = form
      editing.content = "newer incomplete editing"
      _ = try await backend.saveComposer(request: TeraComposerSaveRequest(
        scope: scope, id: id, expectedRevision: 1, editSequence: 2, form: editing
      ))
      let replay = try await backend.prepareSubmission(request: request, media: [])
      XCTAssertEqual(replay, first, "Committed replay must not reopen local media")
      committed.append(first)
    }
    let legacy = try await backend.draftHeads(limit: 100)
    XCTAssertTrue(legacy.isEmpty)
    let page = try await backend.listSubmissions(scope: scope, limit: 100, cursor: nil)
    XCTAssertEqual(page.entries.count, 5)
    XCTAssertEqual(Set(committed.map(\.operationID)).count, 5)
    _ = try await runtime.shutdown()
    try await assertReconstruction(fixture, committed: committed)
  }

  func testProductionClientRetainsOriginalQueuedStatusAfterUnavailableSignerAndRestart() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let signer = ComposerForbiddenSigner()
    let client = TeraRuntimeClient.production()
    let configuration = configuration(fixture, signer: signer)
    _ = try await client.start(configuration: configuration)
    let source = try await client.saveComposer(request: TeraComposerSaveRequest(
      scope: scope, id: client.reserveComposerID(), expectedRevision: nil, editSequence: 1,
      form: form(.createUpdate, media: fixture.media)
    ))
    let request = try await TeraSubmissionRequest(commandID: client.reserveSubmissionID(),
                                                  scope: scope, composerID: source.draft.id, expectedRevision: 1)
    let prepared = try await client.prepareSubmission(request: request, media: [])
    do {
      _ = try await client.advanceSubmission(request: request, expectedRevision: prepared.revision)
      XCTFail("The fixture refuses every signing request")
    } catch {}
    let queued = try await client.submissionStatus(request: request)
    XCTAssertEqual(queued.operationID, prepared.operationID)
    XCTAssertEqual(queued.captured, prepared.captured)
    XCTAssertEqual(queued.settlement.signed, 0)
    _ = try await client.stop()
    _ = try await client.start(configuration: configuration)
    let replay = try await client.recoverSubmission(request: request)
    XCTAssertEqual(replay, queued)
    _ = try await client.stop()
  }

  func testGeneratedOperationRejectsWrongCaptureRequestSchemaAndSettlementVersion() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    try runtime.configureSimulatorRelays(loopbackRelays: ["ws://127.0.0.1:19999"])
    let source = try TeraComposerSaveRequest(scope: scope, id: composerReserveId().id,
                                             expectedRevision: nil, editSequence: 1, form: form(.createUpdate, media: fixture.media))
    _ = try await runtime.composerSave(request: source.generatedValue)
    let request = try TeraSubmissionRequest(commandID: submissionReserveId().id,
                                            scope: scope, composerID: source.id, expectedRevision: 1)
    let wire = try await runtime.submissionPrepare(request: request.generatedValue, media: [])
    _ = try TeraGeneratedSubmission.operation(wire, expected: request)
    var wrong = wire
    wrong.captured.revision = 2
    XCTAssertThrowsError(try TeraGeneratedSubmission.operation(wrong, expected: request))
    wrong = wire
    wrong.request.commandId = String(repeating: "b", count: 32)
    XCTAssertThrowsError(try TeraGeneratedSubmission.operation(wrong, expected: request))
    wrong = wire
    wrong.schemaVersion = 2
    XCTAssertThrowsError(try TeraGeneratedSubmission.operation(wrong, expected: request))
    wrong = wire
    wrong.settlement.schemaVersion = 2
    XCTAssertThrowsError(try TeraGeneratedSubmission.operation(wrong, expected: request))
    _ = try await runtime.shutdown()
  }

  private func assertReconstruction(_ fixture: MediaOwnershipFixture, committed: [TeraSubmissionStatus]) async throws {
    let reopened = try await fixture.runtime()
    try reopened.configureSimulatorRelays(loopbackRelays: ["ws://127.0.0.1:19998"])
    let recovered = TeraGeneratedRuntimeBackend(runtime: reopened)
    for original in committed {
      let status = try await recovered.recoverSubmission(request: original.request)
      XCTAssertEqual(status, original)
      let editing = try await recovered.loadComposer(scope: scope, id: original.captured.id)
      XCTAssertEqual(editing.form.content, "newer incomplete editing")
      XCTAssertEqual(editing.revision, 2)
    }
    _ = try await reopened.shutdown()
  }

  private func form(_ type: TeraAddCommandType, media: TeraPreparedMedia) -> TeraComposerForm {
    var value = TeraAddForm.empty(type)
    value.content = "PRIVATE captured publication"
    switch type {
    case .createUpdate, .createAsk: break
    case .createPhotoUpdate: value.media = [media]
    case .createEvent:
      value.identifier = "fixture-market"
      value.title = "Market"
      value.location = "Town square"
      value.eventTiming = .allDay
      value.eventStartDate = "2026-10-01"
      value.eventEndDate = "2026-10-02"
      value.eventStartUnixSeconds = nil
      value.eventEndUnixSeconds = nil
      value.eventTimezone = nil
    case .createFoodAvailability:
      value.identifier = "fixture-carrots"
      value.title = "Carrots"
      value.summary = "Fresh carrots"
      value.location = "Town square"
      value.priceAmount = "3.25"
      value.currency = "CAD"
      value.unit = "lb"
    }
    return TeraComposerForm(editing: value)
  }

  private func configuration(_ fixture: MediaOwnershipFixture, signer: ComposerForbiddenSigner) -> TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(applicationSupportDirectory: fixture.root.path, publicKeyHex: scope.authorPublicKey,
                                   sourceGenerationHex: String(repeating: "04", count: 32), sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
                                   protectedData: .available, networkProfile: .publicNetwork, writableRelays: ["wss://relay.example"], blossom: nil,
                                   app: TeraRuntimeAppMetadata(bundleIdentifier: "test.submission", version: "1", buildNumber: "1", buildSHA: nil),
                                   signerGeneration: "submission-test", signer: signer, adoptBootstrapSettings: false)
  }
}
