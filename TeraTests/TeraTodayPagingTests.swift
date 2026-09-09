@testable import TeraApp
import XCTest

@MainActor
final class TeraTodayPagingTests: XCTestCase {
  func testStalePagePreservesCardsAndIdentityUntilExplicitRefresh() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    let cards = store.cards
    let generation = store.scopeGeneration
    let pause = await backend.pause(.page, failure: stale())
    let loading = Task { await store.loadNextPage() }
    await pause.entered.wait()
    XCTAssertEqual(store.cards, cards)
    await pause.resume.open()
    await loading.value
    XCTAssertEqual(store.cards, cards)
    XCTAssertEqual(store.scopeGeneration, generation)
    XCTAssertEqual(store.presentation.content, .available)
    XCTAssertEqual(store.presentation.refresh, .completed)
    XCTAssertTrue(store.presentation.readFailure?.requiresRefresh == true)
    XCTAssertFalse(store.isLoadingNextPage)
    XCTAssertFalse(store.canLoadNextPage)
    let before = await backend.counts
    for _ in 0 ..< 3 {
      await store.loadNextPage()
    }
    let after = await backend.counts
    XCTAssertEqual(before, after)
    await assertExplicitRefresh(backend, store: store, retained: cards)
    _ = try await client.stop()
  }

  func testMismatchedAsOfRefusesAppendAndStopsTheOldCursor() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    await backend.setPage(TeraTodayPage(asOfUnixSeconds: 2, items: [TeraScopeFixtures.card("wrong")], nextCursor: "wrong"), cursor: "next")
    let cards = store.cards
    let generation = store.scopeGeneration
    await store.loadNextPage()
    XCTAssertEqual(store.cards, cards)
    XCTAssertEqual(store.scopeGeneration, generation)
    XCTAssertTrue(store.presentation.readFailure?.requiresRefresh == true)
    XCTAssertFalse(store.canLoadNextPage)
    XCTAssertFalse(store.isLoadingNextPage)
    _ = try await client.stop()
  }

  func testLateStaleFailureCannotDisableTheNewContextCursor() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    let pause = await backend.pause(.page, failure: stale())
    let old = Task { await store.loadNextPage() }
    await pause.entered.wait()
    store.configure(snapshot: TeraScopeFixtures.snapshot(account: "b", relay: "second"))
    await backend.setPage(page(["new"], next: "new-next"))
    await store.reload(refreshProjection: false)
    await pause.resume.open()
    await old.value
    XCTAssertEqual(store.cards.map(\.id), ["new"])
    XCTAssertTrue(store.canLoadNextPage)
    XCTAssertNil(store.presentation.readFailure)
    _ = try await client.stop()
  }

  func testStalenessUsesTypedRecoveryAndKeepsAccessibleGuidance() {
    let failure = TeraTodayFailure(TeraRuntimeClientError.today(stale()))
    XCTAssertTrue(failure.requiresRefresh)
    XCTAssertEqual(failure.readStatus, TeraUserMessages.text(.todayChanged))
    let storage = TeraRuntimeFailure.local(operation: "test", code: "today_storage_failed", safeMessage: "stale cursor")
    XCTAssertFalse(TeraTodayFailure(storage).requiresRefresh)
    var presentation = TeraTodayPresentation()
    presentation.acceptPage(count: 2)
    presentation.failRead(failure)
    XCTAssertEqual(presentation.content, .available)
    XCTAssertTrue(presentation.accessibilityStatus.contains(failure.message))
    XCTAssertFalse(presentation.accessibilityStatus.contains("Saved posts could not be read."))
  }

  private func assertExplicitRefresh(_ backend: TeraScopeBackend, store: TeraTodayStore, retained: [TeraTodayCard]) async {
    await backend.setPage(page(["one", "new"], next: "fresh-next"))
    await backend.setPage(page(["last"]), cursor: "fresh-next")
    let pause = await backend.pause(.page)
    let refresh = Task { await store.reload(refreshProjection: false) }
    await pause.entered.wait()
    XCTAssertEqual(store.cards, retained)
    await pause.resume.open()
    await refresh.value
    XCTAssertEqual(store.cards.map(\.id), ["one", "new"])
    XCTAssertNil(store.presentation.readFailure)
    XCTAssertTrue(store.canLoadNextPage)
    await store.loadNextPage()
    XCTAssertEqual(store.cards.map(\.id), ["one", "new", "last"])
    XCTAssertFalse(store.canLoadNextPage)
  }

  private func makeStore(_ backend: TeraScopeBackend) async throws -> (TeraRuntimeClient, TeraTodayStore) {
    await backend.setPage(page(["one", "two"], next: "next"))
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.reload()
    return (client, store)
  }

  private func page(_ ids: [String], next: String? = nil) -> TeraTodayPage {
    TeraTodayPage(asOfUnixSeconds: 1, items: ids.map(TeraScopeFixtures.card), nextCursor: next)
  }

  private func stale() -> TeraRuntimeFailure {
    .local(operation: "test", code: "today_cursor_invalid", safeMessage: "Controlled stale page.")
  }
}
