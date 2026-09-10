import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraComposerDurabilityBoundaryTests: XCTestCase {
  func testTypedStorageAndProtectedDataFailuresKeepEditingAndNeverClaimSaved() async throws {
    // Actual SQLite COMMIT/capacity/busy faults are qualified by the owning
    // storage crate. This fixture checks their typed host presentation boundary.
    let cases: [(String, TeraUserMessageKey)] = [
      ("storage_space_insufficient", .storageFull),
      ("database_busy", .secureStateUnavailable),
      ("composer_storage_failed", .secureStateUnavailable),
      ("protected_data_unavailable", .protectedDataUnavailable),
    ]
    for (code, message) in cases {
      let backend = try TeraScopeBackend()
      let client = try await TeraScopeFixtures.client(backend)
      let store = TeraAddStore(runtimeClient: client)
      store.configure(snapshot: TeraScopeFixtures.snapshot())
      await store.start()
      store.updateForm(\.content, "acknowledged baseline")
      await store.save()
      let baseline = try XCTUnwrap(store.savedComposer)
      let failure = TeraRuntimeFailure.local(operation: "test.composer", code: code,
                                             safeMessage: "Controlled unavailable storage.")
      let pause = await backend.pause(.composer, failure: failure)
      store.updateForm(\.content, "preserve this incomplete edit")
      let save = Task { await store.save() }
      await pause.entered.wait()
      XCTAssertEqual(store.savedComposer, baseline)
      XCTAssertNil(store.message)
      await pause.resume.open()
      await save.value
      XCTAssertEqual(store.composerState, .failed, code)
      XCTAssertEqual(store.lastFailureCode, code)
      XCTAssertEqual(store.message, TeraUserMessages.text(message))
      XCTAssertEqual(store.form.content, "preserve this incomplete edit")
      XCTAssertEqual(store.savedComposer, baseline)
      store.updateForm(\.content, "newest retained edit")
      await store.save()
      let retried = try XCTUnwrap(store.savedComposer)
      XCTAssertEqual(retried.id, baseline.id)
      XCTAssertEqual(retried.revision, baseline.revision + 1)
      XCTAssertEqual(retried.form.content, "newest retained edit")
      XCTAssertEqual(store.composerState, .saved)
      XCTAssertEqual(store.message, "Draft saved on this device.")
      store.stop()
      _ = try await client.stop()
    }
  }

  func testUnavailableProtectedDataRejectsProductionStartupAndReopensBaselineAfterUnlock() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let signer = ComposerForbiddenSigner()
    let client = TeraRuntimeClient.production()
    let available = configuration(fixture, signer: signer, protectedData: .available)
    let snapshot = try await client.start(configuration: available)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: snapshot)
    await store.start()
    store.updateForm(\.content, "committed before protected data became unavailable")
    await store.save()
    let baseline = try XCTUnwrap(store.savedComposer)
    store.stop()
    _ = try await client.stop()
    do {
      _ = try await client.start(configuration: configuration(fixture, signer: signer, protectedData: .unavailable))
      XCTFail("Unavailable protected data must deny the actual runtime startup.")
    } catch let TeraRuntimeClientError.startup(failure) {
      XCTAssertEqual(failure.code, "protected_data_unavailable")
      XCTAssertEqual(TeraUserMessages.key(for: failure, fallback: .startupFailed), .protectedDataUnavailable)
    }
    do {
      _ = try await client.saveComposer(request: TeraComposerSaveRequest(
        scope: baseline.scope, id: baseline.id, expectedRevision: baseline.revision,
        editSequence: baseline.editSequence + 1, form: baseline.form
      ))
      XCTFail("A stopped runtime must never acknowledge a write.")
    } catch { XCTAssertEqual(error as? TeraRuntimeClientError, .notRunning) }
    _ = try await client.start(configuration: available)
    let reopened = try await client.loadComposer(scope: baseline.scope, id: baseline.id)
    XCTAssertEqual(reopened, baseline)
    _ = try await client.stop()
    let signingRequests = await signer.requests
    XCTAssertEqual(signingRequests, 0)
  }

  private func configuration(
    _ fixture: MediaOwnershipFixture, signer: ComposerForbiddenSigner, protectedData: TeraProtectedDataState
  ) -> TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: fixture.root.path,
      publicKeyHex: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
      sourceGenerationHex: String(repeating: "04", count: 32),
      sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
      protectedData: protectedData, networkProfile: .publicNetwork,
      writableRelays: ["wss://relay.example"], blossom: nil,
      app: TeraRuntimeAppMetadata(bundleIdentifier: "test.composer-durability", version: "1", buildNumber: "1", buildSHA: nil),
      signerGeneration: "composer-durability", signer: signer, adoptBootstrapSettings: false
    )
  }
}
