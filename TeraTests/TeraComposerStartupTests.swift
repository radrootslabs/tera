import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraComposerStartupTests: XCTestCase {
  func testSlowFailingMediaAndLegacyStartupLeaveEditingAndRecoveryAvailable() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let media = RecoveryMediaFailure()
    let legacy = await backend.pause(.drafts, fails: true)
    let store = TeraAddStore(runtimeClient: client, media: media)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    let startup = Task { await store.start() }
    await media.supportPause.entered.wait()
    await legacy.entered.wait()
    XCTAssertEqual(store.state, .ready)
    XCTAssertTrue(store.canSave)
    await TeraScopeFixtures.eventually { !store.recovery.isLoading }
    XCTAssertNil(store.recovery.composerError)
    store.updateForm(\.content, "editing during media recovery")
    await store.save()
    XCTAssertEqual(store.composerState, .saved)
    await media.supportPause.resume.open()
    await legacy.resume.open()
    await startup.value
    XCTAssertEqual(store.state, .ready)
    XCTAssertEqual(store.form.content, "editing during media recovery")
    store.stop()
    _ = try await client.stop()
  }

  func testReconciliationFailureIsReportedSeparatelyFromReadyComposer() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let media = RecoveryMediaFailure()
    await media.supportPause.resume.open()
    let store = TeraAddStore(runtimeClient: client, media: media)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    XCTAssertEqual(store.state, .ready)
    XCTAssertTrue(store.canSave)
    XCTAssertEqual(store.mediaRecoveryMessage, "Photo recovery needs attention. Saved editing is still available.")
    XCTAssertNil(store.message)
    store.stop()
    _ = try await client.stop()
  }

  func testCancelledSchemaLoadCannotEnableEditorThroughLateReply() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let schema = await backend.pause(.schemas)
    let stores = TeraProductStores(runtimeClient: client)
    stores.configure(snapshot: TeraScopeFixtures.snapshot())
    let startup = Task { await stores.resume() }
    await schema.entered.wait()
    XCTAssertFalse(stores.add.isProductReady)
    XCTAssertTrue(stores.add.schemas.isEmpty)
    stores.stop()
    await startup.value
    await schema.resume.open()
    _ = try await client.stop()
    XCTAssertFalse(stores.add.isProductReady)
    XCTAssertTrue(stores.add.schemas.isEmpty)
    XCTAssertEqual(stores.add.observationState, .stopped)
  }
}

private actor RecoveryMediaFailure: TeraAddMediaHandling {
  let supportPause = ResourceTestPause()
  func support() async throws -> TeraAddMediaSupport {
    await supportPause.wait()
    throw TeraScopeFixtures.failure()
  }

  func reconcileBackgroundUploads(drafts _: [TeraDraftStatus]) throws {
    throw TeraScopeFixtures.failure()
  }

  func importImages(limit _: Int) throws -> [TeraPreparedMedia] {
    throw TeraScopeFixtures.failure()
  }

  func captureImage() throws -> TeraPreparedMedia {
    throw TeraScopeFixtures.failure()
  }

  func open(_: [TeraPreparedMedia]) throws -> TeraOpenedMedia {
    throw TeraScopeFixtures.failure()
  }
}
