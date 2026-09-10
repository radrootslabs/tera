import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraComposerPersistenceFFITests: XCTestCase {
  private let scope = TeraComposerScope(
    authorPublicKey: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798", localNetworkID: "nearby"
  )

  func testProductionClientPreservesIncompleteFieldsExactReceiptsAndRelaunch() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let signer = ComposerForbiddenSigner()
    let configuration = configuration(fixture, signer: signer)
    let client = TeraRuntimeClient.production()
    _ = try await client.start(configuration: configuration)
    let id = try await client.reserveComposerID()
    XCTAssertEqual(id.count, 32)
    var form = partialForm(fixture)
    let first = try await client.saveComposer(request: TeraComposerSaveRequest(
      scope: scope, id: id, expectedRevision: nil, editSequence: .max - 1, form: form
    ))
    XCTAssertEqual(first.draft.scope, scope)
    XCTAssertEqual(first.draft.id, id)
    XCTAssertEqual(first.draft.revision, 1)
    XCTAssertEqual(first.draft.editSequence, .max - 1)
    XCTAssertEqual(first.draft.form, form)
    XCTAssertFalse(first.replayed)
    form.content = "newest incomplete"
    let request = TeraComposerSaveRequest(scope: scope, id: id, expectedRevision: 1, editSequence: .max, form: form)
    let saved = try await client.saveComposer(request: request)
    XCTAssertEqual(saved.draft.revision, 2)
    XCTAssertEqual(saved.draft.editSequence, .max)
    do {
      _ = try await client.saveComposer(request: request)
      XCTFail("A stale revision must not overwrite the durable head.")
    } catch let TeraRuntimeClientError.add(failure) {
      XCTAssertEqual(failure.code, "composer_revision_conflict")
      XCTAssertEqual(failure.recoveryActions, ["reload_composer"])
    }
    try await assertInventory(client, id: id)
    _ = try await client.stop()
    try await assertStopped(client, id: id)
    _ = try await client.start(configuration: configuration)
    let reopened = try await client.loadComposer(scope: scope, id: id)
    XCTAssertEqual(reopened, saved.draft)
    let foreign = TeraComposerScope(authorPublicKey: scope.authorPublicKey, localNetworkID: "elsewhere")
    do {
      _ = try await client.loadComposer(scope: foreign, id: id)
      XCTFail("A foreign context must not reopen the composer.")
    } catch let TeraRuntimeClientError.add(failure) {
      XCTAssertEqual(failure.code, "composer_scope_mismatch")
    }
    _ = try await client.stop()
    let signingRequests = await signer.requests
    XCTAssertEqual(signingRequests, 0)
  }

  func testGeneratedBoundaryRejectsNestedVersionsAndPreservesPartialMediaMetadata() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let form = partialForm(fixture)
    XCTAssertEqual(try form.generatedValue.composerAppValue, form)
    let request = try TeraComposerSaveRequest(scope: scope, id: TeraGeneratedComposer.reserveID(),
                                              expectedRevision: nil, editSequence: 1, form: form).generatedValue
    var wrongRequest = request
    wrongRequest.schemaVersion = 2
    var wrongScope = request
    wrongScope.scope.schemaVersion = 2
    var wrongForm = request
    wrongForm.form.schemaVersion = 2
    var wrongMedia = request
    wrongMedia.form.media[0].schemaVersion = 2
    for invalid in [wrongRequest, wrongScope, wrongForm, wrongMedia] {
      do {
        _ = try await runtime.composerSave(request: invalid)
        XCTFail("Unsupported versions must fail before persistence.")
      } catch let TeraAppError.Failure(report) {
        XCTAssertEqual(report.schemaVersion, 1)
        XCTAssertEqual(report.code, "composer_schema_unsupported")
        XCTAssertFalse(report.retryable)
      }
    }
    XCTAssertThrowsError(try wrongForm.form.composerAppValue)
    XCTAssertThrowsError(try wrongMedia.form.composerAppValue)
    let empty = try await runtime.composerList(schemaVersion: 1, scope: scope.generatedValue, limit: 10, cursor: nil)
    XCTAssertTrue(empty.entries.isEmpty)
    _ = try await runtime.shutdown()
  }

  private func partialForm(_ fixture: MediaOwnershipFixture) -> TeraComposerForm {
    var editing = TeraAddForm(commandType: .createEvent)
    editing.content = "  private incomplete\n\u{0}é  "
    editing.eventTiming = .allDay
    editing.eventStartDate = "2026-09-"
    editing.eventEndDate = ""
    editing.eventTimezone = "Mars/unfinished"
    editing.eventStartUnixSeconds = .max
    editing.priceAmount = "12."
    editing.quantity = "-"
    editing.media = [fixture.media]
    return TeraComposerForm(editing: editing)
  }

  private func assertInventory(_ client: TeraRuntimeClient, id: String) async throws {
    let page = try await client.listComposers(scope: scope, limit: 1)
    XCTAssertEqual(page.scope, scope)
    XCTAssertNil(page.nextCursor)
    guard case let .draft(summary) = try XCTUnwrap(page.entries.first) else {
      return XCTFail("The saved composer must have a summary.")
    }
    XCTAssertEqual(summary.id, id)
    XCTAssertEqual(summary.revision, 2)
    XCTAssertEqual(summary.editSequence, .max)
    let legacy = try await client.draftHeads()
    XCTAssertTrue(legacy.isEmpty)
  }

  private func assertStopped(_ client: TeraRuntimeClient, id: String) async throws {
    do {
      _ = try await client.loadComposer(scope: scope, id: id)
      XCTFail("Stopped clients must refuse composer operations.")
    } catch {
      XCTAssertEqual(error as? TeraRuntimeClientError, .notRunning)
    }
  }

  private func configuration(_ fixture: MediaOwnershipFixture, signer: ComposerForbiddenSigner) -> TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: fixture.root.path, publicKeyHex: scope.authorPublicKey,
      sourceGenerationHex: String(repeating: "04", count: 32), sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
      protectedData: .available, networkProfile: .publicNetwork, writableRelays: ["wss://relay.example"], blossom: nil,
      app: TeraRuntimeAppMetadata(bundleIdentifier: "test.composer", version: "1", buildNumber: "1", buildSHA: nil),
      signerGeneration: "composer-test", signer: signer, adoptBootstrapSettings: false
    )
  }
}

private actor ComposerForbiddenSigner: TeraRuntimeSigner {
  private(set) var requests = 0
  func availability() -> TeraRuntimeSignerAvailability {
    .ready
  }

  func sign(_: TeraRuntimeSigningRequest) -> TeraRuntimeSigningOutcome {
    requests += 1
    return .failed
  }
}
