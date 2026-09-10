import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraDraftRecoveryTests: XCTestCase {
  private let scope = TeraComposerScope(authorPublicKey: String(repeating: "a", count: 64), localNetworkID: "default")

  func testRestoredCounterOverflowFailsWithoutWritingOrClaimingSaved() async throws {
    let storage = ComposerTestStorage()
    let composer = TeraComposerAutosave(persistence: storage.port, delay: {})
    composer.reset(scope: scope)
    let draft = TeraComposerDraft(scope: scope, id: String(repeating: "1", count: 32), revision: 7,
                                  editSequence: .max, form: form("saved"))
    try composer.restore(draft)
    XCTAssertEqual(composer.acknowledged, draft)
    XCTAssertFalse(composer.isDirty)
    composer.change(form("keep this edit"))
    XCTAssertTrue(composer.isDirty)
    XCTAssertEqual(composer.state, .failed)
    do {
      _ = try await composer.save(form("keep this edit"))
      XCTFail("Exhausted edit sequences cannot be saved.")
    } catch {}
    let writes = await storage.requests
    XCTAssertTrue(writes.isEmpty)
    XCTAssertEqual(composer.acknowledged, draft)
  }

  func testInvalidRestoreRetainsAcknowledgmentAndOldWriteCannotReplaceRestoredIdentity() async throws {
    let storage = ComposerTestStorage()
    let composer = TeraComposerAutosave(persistence: storage.port, delay: {})
    composer.reset(scope: scope)
    let pause = await storage.pauseNext()
    composer.change(form("old write"))
    await pause.entered.wait()
    let restored = TeraComposerDraft(scope: scope, id: String(repeating: "2", count: 32), revision: 4,
                                     editSequence: 5, form: form("restored"))
    try composer.restore(restored)
    for invalid in [
      TeraComposerDraft(scope: scope, id: String(repeating: "0", count: 32), revision: 1, editSequence: 1, form: form("invalid")),
      TeraComposerDraft(scope: scope, id: restored.id, revision: 0, editSequence: 1, form: form("invalid")),
      TeraComposerDraft(scope: scope, id: restored.id, revision: .max, editSequence: 1, form: form("invalid")),
      TeraComposerDraft(scope: scope, id: restored.id, revision: 1, editSequence: 0, form: form("invalid")),
      TeraComposerDraft(scope: TeraComposerScope(authorPublicKey: scope.authorPublicKey, localNetworkID: "foreign"),
                        id: restored.id, revision: 1, editSequence: 1, form: form("invalid")),
    ] {
      XCTAssertThrowsError(try composer.restore(invalid))
      XCTAssertEqual(composer.acknowledged, restored)
    }
    await pause.resume.open()
    await storage.completed.wait()
    let confirmed = try await composer.save(restored.form)
    XCTAssertEqual(confirmed, restored)
    XCTAssertEqual(composer.state, .saved)
    let writes = await storage.requests
    XCTAssertEqual(writes.count, 1)
  }

  func testMissingForeignAndLateSelectionsKeepCurrentEditing() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    store.updateForm(\.content, "keep current editing")
    await store.save()
    let original = try XCTUnwrap(store.savedComposer)
    let missing = await store.reopenSaved(.composer(String(repeating: "3", count: 32)))
    XCTAssertFalse(missing)
    XCTAssertEqual(store.form.content, "keep current editing")
    XCTAssertEqual(store.savedComposer, original)
    let foreign = try await client.saveComposer(request: TeraComposerSaveRequest(
      scope: TeraComposerScope(authorPublicKey: scope.authorPublicKey, localNetworkID: "foreign"),
      id: String(repeating: "4", count: 32), expectedRevision: nil, editSequence: 1, form: form("foreign")
    ))
    let refused = await store.reopenSaved(.composer(foreign.draft.id))
    XCTAssertFalse(refused)
    XCTAssertEqual(store.savedComposer, original)
    let pause = await backend.pause(.composerLoad)
    let old = Task { await store.reopenSaved(.composer(original.id)) }
    await pause.entered.wait()
    let snapshot = TeraScopeFixtures.snapshot(account: "b")
    await backend.configure(snapshot)
    store.configure(snapshot: snapshot)
    await store.start()
    store.updateForm(\.content, "new account editing")
    await pause.resume.open()
    let applied = await old.value
    XCTAssertFalse(applied)
    XCTAssertEqual(store.form.content, "new account editing")
    XCTAssertNil(store.savedComposer)
    store.stop()
    _ = try await client.stop()
  }

  func testInventoryReplacesBoundedPagesPreservesRepairsAndIsolatesLegacyFailure() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let entries = (1 ... 100).map { index in
      TeraComposerListEntry.draft(TeraComposerSummary(id: String(format: "%032x", index), revision: 1,
                                                      editSequence: 1, commandType: .createEvent,
                                                      createdAtUnixMilliseconds: 1, updatedAtUnixMilliseconds: 1))
    }
    let repairs: [TeraComposerListEntry] = [
      .repair(draftKey: String(repeating: "0", count: 32), revision: 1, reason: .corruptRecord),
      .repair(draftKey: String(repeating: "f", count: 32), revision: 2, reason: .unsupportedSchema),
    ]
    await backend.setComposerPage(TeraComposerPage(scope: scope, entries: entries, nextCursor: "next"))
    await backend.setComposerPage(TeraComposerPage(scope: scope, entries: repairs, nextCursor: nil), cursor: "next")
    let failure = await backend.pause(.legacyPage, fails: true)
    let recovery = TeraDraftRecoveryStore(client: client)
    recovery.configure(scope: scope)
    recovery.start()
    await failure.entered.wait()
    XCTAssertEqual(recovery.composers, entries)
    await failure.resume.open()
    await TeraScopeFixtures.eventually { !recovery.isLoading }
    XCTAssertNotNil(recovery.legacyError)
    XCTAssertNil(recovery.composerError)
    recovery.moreComposers()
    await TeraScopeFixtures.eventually { !recovery.isLoading }
    XCTAssertEqual(recovery.composers, repairs)
    XCTAssertNil(recovery.composerCursor)
    recovery.start()
    await TeraScopeFixtures.eventually { !recovery.isLoading }
    XCTAssertEqual(recovery.composers, entries)
    XCTAssertNil(recovery.legacyError)
    let limits = await backend.recoveryLimits
    XCTAssertTrue(limits.allSatisfy { $0 == 100 })
    recovery.stop()
    _ = try await client.stop()
  }

  func testOldInventoryPageCannotReplaceChangedScopeAndWorkRemainsBounded() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let old = await backend.pause(.composerList)
    let recovery = TeraDraftRecoveryStore(client: client)
    recovery.configure(scope: scope)
    recovery.start()
    await old.entered.wait()
    for index in 1 ... 100 {
      recovery.configure(scope: TeraComposerScope(authorPublicKey: scope.authorPublicKey, localNetworkID: "context-\(index)"))
      recovery.start()
    }
    let counts = await backend.counts
    XCTAssertEqual(counts[.composerList], 1)
    await old.resume.open()
    await TeraScopeFixtures.eventually { !recovery.isLoading }
    let finalCounts = await backend.counts
    XCTAssertEqual(finalCounts[.composerList], 2)
    XCTAssertEqual(recovery.scope?.localNetworkID, "context-100")
    XCTAssertNil(recovery.composerError)
    XCTAssertTrue(recovery.composers.isEmpty)
    recovery.stop()
    _ = try await client.stop()
  }

  private func form(_ content: String) -> TeraComposerForm {
    var value = TeraComposerForm(commandType: .createEvent)
    value.content = content
    value.eventStartDate = "2026-09-"
    return value
  }

  func testLegacySelectionLoadsOriginalFormAndRejectsForeignAuthor() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    let applied = await store.reopenSaved(.legacy("draft"))
    XCTAssertTrue(applied)
    XCTAssertEqual(store.activeDraft, TeraScopeFixtures.draft("old"))
    XCTAssertEqual(store.form, TeraScopeFixtures.draft("old").form)
    let snapshot = TeraScopeFixtures.snapshot(account: "b")
    await backend.configure(snapshot)
    store.configure(snapshot: snapshot)
    await store.start()
    store.updateForm(\.content, "other account editing")
    let refused = await store.reopenSaved(.legacy("draft"))
    XCTAssertFalse(refused)
    XCTAssertEqual(store.form.content, "other account editing")
    XCTAssertNil(store.activeDraft)
    store.stop()
    _ = try await client.stop()
  }

  func testLegacyRepairContinuationRemainsSeparateFromComposerPage() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let first: [TeraLegacyDraftListEntry] = (1 ... 100).map { index in
      .repair(draftKey: String(format: "%032x", index), revision: 1, reason: .unsupportedSchema)
    }
    let last: [TeraLegacyDraftListEntry] = [.repair(draftKey: String(repeating: "0", count: 32), revision: 2, reason: .needsAttention)]
    await backend.setLegacyPage(TeraLegacyDraftPage(authorPublicKey: scope.authorPublicKey, entries: first, nextCursor: "next"))
    await backend.setLegacyPage(TeraLegacyDraftPage(authorPublicKey: scope.authorPublicKey, entries: last, nextCursor: nil), cursor: "next")
    let recovery = TeraDraftRecoveryStore(client: client)
    recovery.configure(scope: scope)
    recovery.start()
    await TeraScopeFixtures.eventually { !recovery.isLoading }
    XCTAssertEqual(recovery.legacy, first)
    recovery.moreLegacy()
    await TeraScopeFixtures.eventually { !recovery.isLoading }
    XCTAssertEqual(recovery.legacy, last)
    XCTAssertNil(recovery.legacyCursor)
    XCTAssertNil(recovery.composerError)
    XCTAssertNil(recovery.legacyError)
    XCTAssertTrue(recovery.composers.isEmpty)
    recovery.stop()
    _ = try await client.stop()
  }
}
