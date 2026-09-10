@testable import TeraApp
import XCTest

@MainActor
final class TeraScopedAddTests: XCTestCase {
  func testServiceProbeCompletionCannotChangeReplacementScopeOrClearItsProbe() async throws {
    for fails in [false, true] {
      let backend = try TeraScopeBackend()
      let client = try await TeraScopeFixtures.client(backend)
      let store = TeraAddStore(runtimeClient: client)
      store.configure(snapshot: TeraScopeFixtures.snapshot())
      let first = await backend.pause(.probe, fails: fails)
      let old = Task { await store.checkPhotoService() }
      await first.entered.wait()
      let updated = TeraScopeFixtures.snapshot(account: "b", relay: "second")
      await backend.configure(updated)
      store.configure(snapshot: updated)
      let second = await backend.pause(.probe)
      let current = Task { await store.checkPhotoService() }
      await second.entered.wait()
      await first.resume.open()
      await old.value
      XCTAssertTrue(store.isCheckingBlossom)
      XCTAssertNil(store.blossomEvidence)
      XCTAssertNil(store.message)
      await second.resume.open()
      await current.value
      XCTAssertEqual(store.blossomEvidence?.configFingerprint, "second")
      XCTAssertFalse(store.isCheckingBlossom)
      _ = try await client.stop()
    }
  }

  func testLateDraftStartupCannotPopulateAReplacementAccount() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    let pause = await backend.pause(.drafts)
    let old = Task { await store.start() }
    await pause.entered.wait()
    store.configure(snapshot: TeraScopeFixtures.snapshot(account: "b"))
    await backend.setDrafts([TeraScopeFixtures.draft("new")])
    await store.start()
    await pause.resume.open()
    await old.value
    XCTAssertEqual(store.drafts.first?.form?.content, "new")
    XCTAssertEqual(store.state, .ready)
    store.stop()
    _ = try await client.stop()
  }

  func testFormEditDuringSubmitKeepsTheNewFormAndRetainsDurableOldReceipt() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    store.updateForm(\.content, "saved version")
    let pause = await backend.pause(.save)
    let old = Task { await store.submit() }
    await pause.entered.wait()
    store.updateForm(\.content, "new edit")
    await pause.resume.open()
    await old.value
    XCTAssertEqual(store.form.content, "new edit")
    XCTAssertNil(store.activeDraft)
    XCTAssertNil(store.message)
    XCTAssertFalse(store.isWorking)
    let durable = try await client.draftHeads(limit: 100)
    XCTAssertEqual(durable.first?.form?.content, "saved version")
    await store.submit()
    XCTAssertEqual(store.activeDraft?.form?.content, "new edit")
    store.stop()
    _ = try await client.stop()
  }

  func testLateFailureSnapshotCannotWriteItsMessageOrClearANewerOperation() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    let save = await backend.pause(.save, fails: true)
    let old = Task { await store.submit() }
    await save.entered.wait()
    let snapshot = await backend.pause(.snapshot)
    await save.resume.open()
    await snapshot.entered.wait()
    let updated = TeraScopeFixtures.snapshot(account: "b")
    await backend.configure(updated)
    store.configure(snapshot: updated)
    await store.start()
    store.updateForm(\.content, "new account")
    let next = await backend.pause(.save)
    let current = Task { await store.submit() }
    await next.entered.wait()
    await snapshot.resume.open()
    await old.value
    XCTAssertTrue(store.isWorking)
    XCTAssertNil(store.message)
    XCTAssertNil(store.lastFailureCode)
    await next.resume.open()
    await current.value
    XCTAssertEqual(store.activeDraft?.form?.content, "new account")
    store.stop()
    _ = try await client.stop()
  }

  func testOldDraftObserverCannotOverwriteOrReportFailureInNewAccount() async throws {
    for fails in [false, true] {
      let backend = try TeraScopeBackend()
      let client = try await TeraScopeFixtures.client(backend)
      let store = TeraAddStore(runtimeClient: client)
      store.configure(snapshot: TeraScopeFixtures.snapshot())
      await store.start()
      await TeraScopeFixtures.eventually { store.observationState == .active }
      let tokens = await backend.tokens
      let oldToken = try XCTUnwrap(tokens.first)
      let pause = await backend.pause(.drafts, fails: fails)
      await backend.emit(.drafts)
      await pause.entered.wait()
      store.configure(snapshot: TeraScopeFixtures.snapshot(account: "b"))
      await backend.setDrafts([TeraScopeFixtures.draft("new account")])
      await store.start()
      await pause.resume.open()
      await oldToken.cancelled.wait()
      XCTAssertEqual(store.drafts.first?.form?.content, "new account")
      XCTAssertNil(store.message)
      store.stop()
      _ = try await client.stop()
    }
  }

  func testLateStartupInventoryCannotOverwriteANewerObserverSnapshot() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    let schemas = await backend.pause(.schemas)
    let startup = Task { await store.start() }
    await schemas.entered.wait()
    await TeraScopeFixtures.eventually { store.observationState == .active }
    await TeraScopeFixtures.eventually { store.drafts.first?.revision == 1 }
    let pause = await backend.pause(.drafts)
    await schemas.resume.open()
    await pause.entered.wait()
    await backend.setDrafts([TeraScopeFixtures.draft("new revision", revision: 2)])
    await backend.emit(.drafts)
    await TeraScopeFixtures.eventually { store.drafts.first?.revision == 2 }
    await pause.resume.open()
    await startup.value
    XCTAssertEqual(store.drafts.first?.revision, 2)
    XCTAssertEqual(store.state, .ready)
    store.stop()
    _ = try await client.stop()
  }

  func testLateProbeCannotOverwriteNewerServiceSnapshotInTheSameScope() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    await TeraScopeFixtures.eventually { store.observationState == .active }
    let pause = await backend.pause(.probe)
    let old = Task { await store.checkPhotoService() }
    await pause.entered.wait()
    let evidence = TeraScopeFixtures.evidence(observedAt: 2)
    await backend.configure(TeraScopeFixtures.snapshot(evidence: evidence))
    await backend.emit(.settings)
    await TeraScopeFixtures.eventually { store.blossomEvidence == evidence }
    await pause.resume.open()
    await old.value
    XCTAssertEqual(store.blossomEvidence, evidence)
    XCTAssertFalse(store.isCheckingBlossom)
    store.stop()
    _ = try await client.stop()
  }

  func testLateFailureSnapshotCannotOverwriteEditedFormMessage() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    let save = await backend.pause(.save, fails: true)
    let old = Task { await store.submit() }
    await save.entered.wait()
    let snapshot = await backend.pause(.snapshot)
    await save.resume.open()
    await snapshot.entered.wait()
    store.updateForm(\.content, "New edit while error recovery waits")
    await snapshot.resume.open()
    await old.value
    XCTAssertNil(store.message)
    XCTAssertNil(store.lastFailureCode)
    XCTAssertEqual(store.form.content, "New edit while error recovery waits")
    XCTAssertFalse(store.isWorking)
    store.stop()
    _ = try await client.stop()
  }
}
