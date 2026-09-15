import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

@MainActor
final class TeraComposerMediaOwnershipTests: XCTestCase {
  func testLateFileConfirmationCannotAcknowledgeNewerEditing() async throws {
    let storage = ComposerTestStorage()
    let pause = ResourceTestPause()
    var persistence = storage.port
    persistence.confirm = { _ in await pause.wait() }
    let autosave = TeraComposerAutosave(persistence: persistence, delay: {})
    autosave.reset(scope: TeraComposerScope(authorPublicKey: String(repeating: "a", count: 64), localNetworkID: "default"))
    var first = TeraComposerForm(commandType: .createUpdate)
    first.content = "first"
    let original = first
    let saving = Task { try await autosave.save(original) }
    await pause.entered.wait()
    first.content = "newer editing"
    autosave.change(first)
    await pause.resume.open()
    do { _ = try await saving.value; XCTFail("Old file proof must not acknowledge new editing") } catch {}
    let latest = try await autosave.save(first)
    XCTAssertEqual(latest.form.content, "newer editing")
    XCTAssertEqual(autosave.state, .saved)
    autosave.stop()
  }

  func testMissingCorruptSymlinkAndLockedFilesCannotCommitReferences() async throws {
    for damage in ["missing", "corrupt", "symlink", "locked"] {
      let fixture = try OfflineMediaFixture()
      defer { fixture.remove() }
      let client = TeraRuntimeClient.production()
      let snapshot = try await client.start(configuration: fixture.configuration(ComposerForbiddenSigner()))
      let coordinator = fixture.coordinator()
      let request = try await request(client, coordinator: coordinator, snapshot: snapshot)
      let item = try XCTUnwrap(request.form.media.first)
      let path = fixture.roots.stagedBlobsRoot.appendingPathComponent(item.sha256)
      let original = try Data(contentsOf: path)
      try damageFile(path, bytes: original, damage: damage, root: fixture.runtime.root)
      let persistence = TeraComposerPersistence(client: client).protectingMedia(coordinator)
      do {
        _ = try await persistence.save(request)
        XCTFail("Unowned bytes must not be acknowledged: \(damage)")
      } catch {}
      let page = try await client.listComposers(scope: request.scope)
      XCTAssertTrue(page.entries.isEmpty, damage)
      if damage == "locked" {
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path.path)
      }
      _ = try await client.stop()
    }
  }

  func testInterruptedBeforeDatabaseCommitRetainsIdentifiableOrphanAndRetries() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let client = TeraRuntimeClient.production()
    let snapshot = try await client.start(configuration: fixture.configuration(ComposerForbiddenSigner()))
    let coordinator = fixture.coordinator()
    let request = try await request(client, coordinator: coordinator, snapshot: snapshot)
    let path = try path(request, fixture: fixture)
    let bytes = try Data(contentsOf: path)
    // Inject the storage result at the file/DB seam. Producer tests separately
    // exercise write/flush faults; this is not a physical disk exhaustion claim.
    let failed = TeraComposerPersistence(reserve: { request.id }, save: { _ in throw POSIXError(.ENOSPC) },
                                         load: { try await client.loadComposer(scope: $0, id: $1) }).protectingMedia(coordinator)
    do { _ = try await failed.save(request); XCTFail("Full storage must fail acknowledgment") } catch {}
    XCTAssertEqual(try Data(contentsOf: path), bytes)
    let absent = try await client.listComposers(scope: request.scope)
    XCTAssertTrue(absent.entries.isEmpty)
    _ = try await client.stop()
    _ = try await client.start(configuration: fixture.configuration(ComposerForbiddenSigner()))
    let restarted = TeraComposerPersistence(client: client).protectingMedia(fixture.coordinator())
    let saved = try await restarted.save(request)
    XCTAssertEqual(saved.draft.form, request.form)
    XCTAssertEqual(try Data(contentsOf: path), bytes)
    _ = try await client.stop()
  }

  func testLostDatabaseReplyReconcilesExactReferenceWithoutSecondWrite() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let client = TeraRuntimeClient.production()
    let snapshot = try await client.start(configuration: fixture.configuration(ComposerForbiddenSigner()))
    let coordinator = fixture.coordinator()
    let request = try await request(client, coordinator: coordinator, snapshot: snapshot)
    let unknown = TeraComposerPersistence(reserve: { request.id }, save: { value in
      _ = try await client.saveComposer(request: value)
      throw TeraComposerAcknowledgment.unconfirmed
    }, load: { try await client.loadComposer(scope: $0, id: $1) }).protectingMedia(coordinator)
    let autosave = TeraComposerAutosave(persistence: unknown)
    autosave.reset(scope: request.scope)
    do { _ = try await autosave.save(request.form); XCTFail("Lost reply is initially unknown") } catch {}
    XCTAssertEqual(autosave.state, .failed)
    XCTAssertNil(autosave.acknowledged)
    let recovered = try await autosave.save(request.form)
    XCTAssertEqual(recovered.revision, 1)
    XCTAssertEqual(autosave.state, .saved)
    let page = try await client.listComposers(scope: request.scope)
    XCTAssertEqual(page.entries.count, 1)
    autosave.stop()
    _ = try await client.stop()
  }

  func testMissingAfterCommitBlocksAcknowledgmentAndRestartUntilBytesRecover() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let client = TeraRuntimeClient.production()
    let snapshot = try await client.start(configuration: fixture.configuration(ComposerForbiddenSigner()))
    let coordinator = fixture.coordinator()
    let request = try await request(client, coordinator: coordinator, snapshot: snapshot)
    let path = try path(request, fixture: fixture)
    let bytes = try Data(contentsOf: path)
    let interrupted = TeraComposerPersistence(reserve: { request.id }, save: { value in
      let receipt = try await client.saveComposer(request: value)
      try FileManager.default.removeItem(at: path)
      return receipt
    }, load: { try await client.loadComposer(scope: $0, id: $1) }).protectingMedia(coordinator)
    do { _ = try await interrupted.save(request); XCTFail("Missing post-commit bytes cannot report saved") } catch {}
    let raw = try await client.loadComposer(scope: request.scope, id: request.id)
    XCTAssertEqual(raw.form, request.form)
    _ = try await client.stop()
    let restarted = try await client.start(configuration: fixture.configuration(ComposerForbiddenSigner()))
    let store = TeraAddStore(runtimeClient: client, media: fixture.coordinator())
    store.configure(snapshot: restarted)
    await store.start()
    let refused = await store.reopenSaved(.composer(request.id))
    XCTAssertFalse(refused)
    XCTAssertNil(store.savedComposer)
    try bytes.write(to: path)
    let reopened = await store.reopenSaved(.composer(request.id))
    XCTAssertTrue(reopened)
    XCTAssertEqual(store.savedComposer, raw)
    try FileManager.default.removeItem(at: path)
    await store.save()
    XCTAssertEqual(store.composerState, .failed)
    XCTAssertNotEqual(store.message, "Draft saved on this device.")
    store.stop()
    _ = try await client.stop()
  }

  func testSharedHashSurvivesFailedOtherOwnerAndCachePurge() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let client = TeraRuntimeClient.production()
    let snapshot = try await client.start(configuration: fixture.configuration(ComposerForbiddenSigner()))
    let coordinator = fixture.coordinator()
    let first = try await request(client, coordinator: coordinator, snapshot: snapshot)
    let persistence = TeraComposerPersistence(client: client).protectingMedia(coordinator)
    let saved = try await persistence.save(first)
    let second = try await TeraComposerSaveRequest(scope: first.scope, id: client.reserveComposerID(),
                                                   expectedRevision: 1, editSequence: 1, form: first.form)
    do { _ = try await persistence.save(second); XCTFail("Missing owner revision must fail") } catch {}
    try FileManager.default.removeItem(at: fixture.roots.cacheRoot)
    if FileManager.default.fileExists(atPath: fixture.roots.temporaryRoot.path) {
      try FileManager.default.removeItem(at: fixture.roots.temporaryRoot)
    }
    _ = try await client.stop()
    _ = try await client.start(configuration: fixture.configuration(ComposerForbiddenSigner()))
    let recovered = try await TeraComposerPersistence(client: client).protectingMedia(fixture.coordinator())
      .load(first.scope, first.id)
    XCTAssertEqual(recovered, saved.draft)
    _ = try await client.stop()
  }

  func testEvictableRootsAndAbsentMediaOwnerFailClosed() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let client = TeraRuntimeClient.production()
    let snapshot = try await client.start(configuration: fixture.configuration(ComposerForbiddenSigner()))
    let request = try await request(client, coordinator: fixture.coordinator(), snapshot: snapshot)
    let legacy = try RadrootsAppleFileRoots(appIdentifier: fixture.roots.appIdentifier,
                                            dataRoot: fixture.roots.dataRoot, cacheRoot: fixture.roots.cacheRoot,
                                            temporaryRoot: fixture.roots.temporaryRoot)
    XCTAssertThrowsError(try TeraComposerMediaOwnership.confirm(request.form.media, roots: legacy))
    do {
      _ = try await TeraComposerPersistence(client: client).protectingMedia(nil).save(request)
      XCTFail("Media needs an explicit durable file owner")
    } catch {}
    let page = try await client.listComposers(scope: request.scope)
    XCTAssertTrue(page.entries.isEmpty)
    _ = try await client.stop()
  }

  private func request(_ client: TeraRuntimeClient, coordinator: TeraAddMediaCoordinator,
                       snapshot: TeraRuntimeSnapshot) async throws -> TeraComposerSaveRequest
  {
    var editing = TeraAddPresentation.newForm(type: .createPhotoUpdate, identifier: { "draft" }, clock: .system)
    editing.media = try await coordinator.importImages(limit: 1)
    let configuration = TeraPresentationConfiguration(snapshot: snapshot)
    return try await TeraComposerSaveRequest(
      scope: TeraComposerScope(authorPublicKey: configuration.publicKey, localNetworkID: configuration.context.id),
      id: client.reserveComposerID(), expectedRevision: nil, editSequence: 1, form: TeraComposerForm(editing: editing)
    )
  }

  private func path(_ request: TeraComposerSaveRequest, fixture: OfflineMediaFixture) throws -> URL {
    try fixture.roots.stagedBlobsRoot.appendingPathComponent(XCTUnwrap(request.form.media.first).sha256)
  }

  private func damageFile(_ path: URL, bytes: Data, damage: String, root: URL) throws {
    switch damage {
    case "missing": try FileManager.default.removeItem(at: path)
    case "corrupt": try Data(repeating: 0, count: bytes.count).write(to: path)
    case "locked": try FileManager.default.setAttributes([.posixPermissions: 0], ofItemAtPath: path.path)
    default:
      let outside = root.appendingPathComponent("outside")
      try bytes.write(to: outside)
      try FileManager.default.removeItem(at: path)
      try FileManager.default.createSymbolicLink(at: path, withDestinationURL: outside)
    }
  }
}
