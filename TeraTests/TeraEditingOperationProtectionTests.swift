import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraEditingOperationProtectionTests: XCTestCase {
  func testNewSavesExistingStrictRevisionBeforeReplacingItsEditing() async throws {
    let backend = AddBackend(advanceOffline: true)
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "original")
    await store.submit()
    let source = try XCTUnwrap(store.activeDraft)
    await store.retractAndRevise(TeraAddStoreTests.card(localOperationID: source.id))
    store.updateForm(\.content, "corrected revision")
    store.newDraft(type: .createAsk)
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    XCTAssertFalse(store.protection.failed)
    XCTAssertEqual(store.form.commandType, .createAsk)
    XCTAssertEqual(store.form.content, "")
    let saved = try XCTUnwrap(store.drafts.first(where: \.isRevision))
    XCTAssertEqual(saved.form?.content, "corrected revision")
    XCTAssertEqual(saved.state, .draft)
    let count = await backend.revisionPlanCount()
    XCTAssertEqual(count, 1)
    store.stop()
    _ = try await client.stop()
  }

  func testFailedPreservationPreventsTodayRevisionAndRetractionEffects() async throws {
    let backend = AddBackend(saveFailure: TeraScopeFixtures.failure())
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "keep exact editing")
    let editing = store.form
    await store.retract(TeraAddStoreTests.card())
    XCTAssertTrue(store.protection.failed)
    XCTAssertEqual(store.form, editing)
    XCTAssertNil(store.activeDraft)
    let retraction = await backend.lastRetraction()
    XCTAssertNil(retraction)
    store.protection.cancel()
    await store.retractAndRevise(TeraAddStoreTests.card(localOperationID: "missing"))
    XCTAssertTrue(store.protection.failed)
    XCTAssertEqual(store.form, editing)
    XCTAssertNil(store.activeDraft)
    let count = await backend.revisionPlanCount()
    XCTAssertEqual(count, 0)
    store.stop()
    _ = try await client.stop()
  }

  func testAbandonedFailedChoiceDoesNotRetainStoppedStore() async throws {
    let backend = AddBackend(saveFailure: TeraScopeFixtures.failure())
    let client = try await TeraAddStoreTests.startedClient(backend)
    var store: TeraAddStore? = TeraAddStore(runtimeClient: client)
    weak var weakStore = store
    await store?.configure(snapshot: backend.snapshot())
    await store?.start()
    store?.updateForm(\.content, "unsaved")
    store?.newDraft()
    await TeraScopeFixtures.eventually { store?.protection.failed == true }
    store?.suspend()
    store = nil
    await TeraScopeFixtures.eventually { weakStore == nil }
    _ = try await client.stop()
  }
}
