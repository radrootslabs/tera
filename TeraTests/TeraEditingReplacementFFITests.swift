import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraEditingReplacementFFITests: XCTestCase {
  func testNewAndTypeReplacementPreserveAcknowledgedIncompleteEditingAcrossProductionRelaunch() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let signer = ComposerForbiddenSigner()
    let configuration = configuration(fixture, signer: signer)
    let client = TeraRuntimeClient.production()
    let snapshot = try await client.start(configuration: configuration)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: snapshot)
    await store.start()
    store.selectType(.createEvent)
    store.updateForm(\.eventStartDate, "2026-09-")
    store.updateForm(\.content, "  unfinished event  ")
    await store.save()
    let original = try XCTUnwrap(store.savedComposer)
    store.updateForm(\.content, "  final incomplete event  ")
    let exact = TeraComposerForm(editing: store.form)
    store.selectType(.createFoodAvailability)
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    XCTAssertFalse(store.protection.failed)
    XCTAssertEqual(store.form.commandType, .createFoodAvailability)
    let saved = try await client.loadComposer(scope: original.scope, id: original.id)
    XCTAssertEqual(saved.form, exact)
    XCTAssertGreaterThan(saved.revision, original.revision)
    store.updateForm(\.priceAmount, "1.")
    store.updateForm(\.quantity, "unfinished")
    await store.save()
    let second = try XCTUnwrap(store.savedComposer)
    XCTAssertNotEqual(second.id, original.id)
    store.updateForm(\.content, "incomplete availability")
    let availability = TeraComposerForm(editing: store.form)
    store.newDraft()
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    XCTAssertEqual(store.form.content, "")
    store.stop()
    _ = try await client.stop()
    _ = try await client.start(configuration: configuration)
    let firstRecovered = try await client.loadComposer(scope: saved.scope, id: saved.id)
    let secondRecovered = try await client.loadComposer(scope: second.scope, id: second.id)
    XCTAssertEqual(firstRecovered, saved)
    XCTAssertEqual(secondRecovered.form, availability)
    let operations = try await client.legacyDraftPage()
    XCTAssertTrue(operations.entries.isEmpty)
    let signingRequests = await signer.requests
    XCTAssertEqual(signingRequests, 0)
    _ = try await client.stop()
  }

  private func configuration(_ fixture: MediaOwnershipFixture, signer: ComposerForbiddenSigner) -> TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: fixture.root.path,
      publicKeyHex: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
      sourceGenerationHex: String(repeating: "04", count: 32), sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
      protectedData: .available, networkProfile: .publicNetwork, writableRelays: [], blossom: nil,
      app: TeraRuntimeAppMetadata(bundleIdentifier: "test.editing", version: "1", buildNumber: "1", buildSHA: nil),
      signerGeneration: "editing-test", signer: signer, adoptBootstrapSettings: false
    )
  }
}
