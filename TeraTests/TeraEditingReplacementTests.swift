import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraEditingReplacementTests: XCTestCase {
  func testCleanTypeChangeIsImmediateAndDirtyTypeChangePreservesExactAcknowledgment() async throws {
    let fixture = try await started()
    let backend = fixture.backend
    let client = fixture.client
    let store = fixture.store
    store.selectType(.createEvent)
    XCTAssertEqual(store.form.commandType, .createEvent)
    store.updateForm(\.eventStartDate, "2026-09-")
    store.updateForm(\.content, "  incomplete event  ")
    await store.save()
    let original = try XCTUnwrap(store.savedComposer)
    let pause = await backend.pause(.composer)
    store.updateForm(\.content, "  newer event  ")
    let editing = TeraComposerForm(editing: store.form)
    store.selectType(.createAsk)
    await pause.entered.wait()
    XCTAssertEqual(TeraComposerForm(editing: store.form), editing)
    store.selectType(.createFoodAvailability)
    await pause.resume.open()
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    XCTAssertEqual(store.form.commandType, .createAsk)
    XCTAssertEqual(store.form.content, "")
    let saved = try await client.loadComposer(scope: original.scope, id: original.id)
    XCTAssertEqual(saved.form, editing)
    XCTAssertEqual(saved.revision, original.revision + 1)
    XCTAssertGreaterThan(saved.editSequence, original.editSequence)
    await store.save()
    XCTAssertNotEqual(store.savedComposer?.id, original.id)
    store.stop()
    _ = try await client.stop()
  }

  func testNewerEditingWhileNewSaveCompletesCancelsReplacementAndKeepsNewestForm() async throws {
    let fixture = try await started()
    let backend = fixture.backend
    let client = fixture.client
    let store = fixture.store
    let first = await backend.pause(.composer)
    store.updateForm(\.content, "first")
    store.newDraft(type: .createEvent)
    await first.entered.wait()
    let second = await backend.pause(.composer)
    store.updateForm(\.content, "newer while saving")
    await first.resume.open()
    await second.entered.wait()
    XCTAssertEqual(store.form.content, "newer while saving")
    XCTAssertEqual(store.form.commandType, .createUpdate)
    await second.resume.open()
    await TeraScopeFixtures.eventually { !store.isWorking }
    XCTAssertEqual(store.form.content, "newer while saving")
    XCTAssertEqual(store.savedComposer?.form.content, "newer while saving")
    XCTAssertFalse(store.protection.failed)
    store.stop()
    _ = try await client.stop()
  }

  func testSaveFailureKeepEditingAndDiscardPreserveLastDurableDraft() async throws {
    let fixture = try await started()
    let backend = fixture.backend
    let client = fixture.client
    let store = fixture.store
    store.updateForm(\.content, "durable")
    await store.save()
    let original = try XCTUnwrap(store.savedComposer)
    let failure = await backend.pause(.composer, fails: true)
    store.updateForm(\.content, "unsaved")
    store.newDraft()
    await failure.entered.wait()
    await failure.resume.open()
    await TeraScopeFixtures.eventually { store.protection.failed }
    XCTAssertEqual(store.form.content, "unsaved")
    store.protection.cancel()
    store.protection.discard()
    XCTAssertEqual(store.form.content, "unsaved")
    let again = await backend.pause(.composer, fails: true)
    store.newDraft(type: .createEvent)
    await again.entered.wait()
    await again.resume.open()
    await TeraScopeFixtures.eventually { store.protection.failed }
    store.protection.discard()
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    XCTAssertEqual(store.form.commandType, .createEvent)
    XCTAssertEqual(store.form.content, "")
    let saved = try await client.loadComposer(scope: original.scope, id: original.id)
    XCTAssertEqual(saved, original)
    store.stop()
    _ = try await client.stop()
  }

  func testSavedSelectionRetryPreservesBufferBeforeLoadingAndSignalsSheetDismissal() async throws {
    let fixture = try await started()
    let backend = fixture.backend
    let client = fixture.client
    let store = fixture.store
    store.updateForm(\.content, "buffer")
    await store.save()
    let original = try XCTUnwrap(store.savedComposer)
    let failure = await backend.pause(.composer, fails: true)
    store.updateForm(\.content, "latest buffer")
    let selection = Task { await store.reopenSaved(.legacy("draft")) }
    await failure.entered.wait()
    await failure.resume.open()
    let applied = await selection.value
    XCTAssertFalse(applied)
    XCTAssertEqual(store.form.content, "latest buffer")
    XCTAssertNil(store.protection.reopened)
    let counts = await backend.counts
    XCTAssertNil(counts[.draftStatus])
    store.protection.retry()
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    XCTAssertEqual(store.form.content, "old")
    XCTAssertNotNil(store.protection.reopened)
    let kept = try await client.loadComposer(scope: original.scope, id: original.id)
    XCTAssertEqual(kept.form.content, "latest buffer")
    await store.save()
    XCTAssertNotEqual(store.savedComposer?.id, original.id)
    store.stop()
    _ = try await client.stop()
  }

  func testScopeChangeCancelsPendingReplacementAndLateSaveCannotReplaceNewAccount() async throws {
    let fixture = try await started()
    let backend = fixture.backend
    let client = fixture.client
    let store = fixture.store
    let pause = await backend.pause(.composer)
    store.updateForm(\.content, "old account")
    store.newDraft(type: .createEvent)
    await pause.entered.wait()
    let snapshot = TeraScopeFixtures.snapshot(account: "b")
    await backend.configure(snapshot)
    store.configure(snapshot: snapshot)
    await store.start()
    store.updateForm(\.content, "current account")
    await pause.resume.open()
    await store.save()
    XCTAssertEqual(store.form.content, "current account")
    XCTAssertEqual(store.form.commandType, .createUpdate)
    XCTAssertEqual(store.savedComposer?.scope.authorPublicKey, String(repeating: "b", count: 64))
    XCTAssertFalse(store.protection.failed)
    store.stop()
    _ = try await client.stop()
  }

  func testLegacyRetryPreservesPriorComposerIdentityAndFailedSelectionHasNoEffects() async throws {
    let fixture = try await started()
    let backend = fixture.backend
    let client = fixture.client
    let store = fixture.store
    store.updateForm(\.content, "first buffer")
    await store.save()
    let original = try XCTUnwrap(store.savedComposer)
    store.updateForm(\.content, "last buffer")
    await store.retry(id: "draft")
    XCTAssertEqual(store.form.content, "old")
    await store.save()
    XCTAssertNotEqual(store.savedComposer?.id, original.id)
    let kept = try await client.loadComposer(scope: original.scope, id: original.id)
    XCTAssertEqual(kept.form.content, "last buffer")
    let failure = await backend.pause(.composer, fails: true)
    store.updateForm(\.content, "keep for cancellation")
    let cancellation = Task { await store.cancel(id: "draft") }
    await failure.entered.wait()
    await failure.resume.open()
    await cancellation.value
    XCTAssertEqual(store.form.content, "keep for cancellation")
    let counts = await backend.counts
    XCTAssertEqual(counts[.draftStatus], 1)
    XCTAssertTrue(store.protection.failed)
    store.stop()
    _ = try await client.stop()
  }

  private struct Fixture {
    let backend: TeraScopeBackend
    let client: TeraRuntimeClient
    let store: TeraAddStore
  }

  func testCancelledSavedSelectionCannotApplyItsLateLoadedForm() async throws {
    let fixture = try await started()
    let store = fixture.store
    store.updateForm(\.content, "keep current")
    await store.save()
    let original = try XCTUnwrap(store.savedComposer)
    var form = TeraComposerForm(commandType: .createAsk)
    form.content = "other saved editing"
    let other = try await fixture.client.saveComposer(request: TeraComposerSaveRequest(
      scope: original.scope, id: String(repeating: "7", count: 32), expectedRevision: nil, editSequence: 1, form: form
    ))
    let pause = await fixture.backend.pause(.composerLoad)
    let selection = Task { await store.reopenSaved(.composer(other.draft.id)) }
    await pause.entered.wait()
    selection.cancel()
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    await pause.resume.open()
    let applied = await selection.value
    XCTAssertFalse(applied)
    XCTAssertEqual(store.form.content, "keep current")
    XCTAssertEqual(store.savedComposer, original)
    XCTAssertNil(store.protection.reopened)
    XCTAssertFalse(store.protection.failed)
    store.stop()
    _ = try await fixture.client.stop()
  }

  private func started() async throws -> Fixture {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    return Fixture(backend: backend, client: client, store: store)
  }
}
