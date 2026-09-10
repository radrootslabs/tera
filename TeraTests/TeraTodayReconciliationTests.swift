@testable import TeraApp
import XCTest

@MainActor
final class TeraTodayReconciliationTests: XCTestCase {
  func testDraftStormRetainsLoadedOrderAndUnchangedCursor() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    await store.loadNextPage()
    let original = store.cards
    let scope = store.scopeGeneration
    let pause = await backend.pause(.reconcile)
    await backend.emit(.drafts)
    await pause.entered.wait()
    for _ in 0 ..< 64 {
      await backend.emit(.media)
      await backend.emit(.drafts)
    }
    XCTAssertEqual(store.cards, original)
    XCTAssertTrue(store.canLoadNextPage)
    await pause.resume.open()
    await TeraScopeFixtures.eventually { !store.presentation.isReading }
    let requests = await backend.reconciliationRequests
    XCTAssertTrue(requests.allSatisfy { $0.cardIDs.count <= 100 })
    XCTAssertEqual(store.cards.map(\.id), ["one", "two", "three"])
    XCTAssertEqual(store.currentCard(id: "two"), original[1])
    XCTAssertEqual(store.scopeGeneration, scope)
    XCTAssertFalse(store.hasPendingContent)
    XCTAssertTrue(store.canLoadNextPage)
    store.stop()
    _ = try await client.stop()
  }

  func testRemovedSelectedCardDisappearsAndNewContentWaitsForExplicitRefresh() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    await store.loadNextPage()
    let scope = store.scopeGeneration
    await backend.setReconciliation(["new", "three", "one"].map(TeraScopeFixtures.card), generation: 2)
    await backend.emit(.today)
    await TeraScopeFixtures.eventually { store.hasPendingContent }
    XCTAssertEqual(store.cards.map(\.id), ["one", "three"])
    XCTAssertNil(store.currentCard(id: "two"))
    XCTAssertNotNil(store.currentCard(id: "three"))
    XCTAssertEqual(store.scopeGeneration, scope)
    XCTAssertFalse(store.canLoadNextPage)
    await backend.setPage(page(["new", "one"], next: "fresh", generation: 2))
    await store.reload(refreshProjection: false)
    XCTAssertEqual(store.cards.map(\.id), ["new", "one"])
    XCTAssertFalse(store.hasPendingContent)
    XCTAssertTrue(store.canLoadNextPage)
    store.stop()
    _ = try await client.stop()
  }

  func testLateOldReconciliationCannotClearNewContext() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    let pause = await backend.pause(.reconcile, fails: true)
    await backend.emit(.today)
    await pause.entered.wait()
    store.configure(snapshot: TeraScopeFixtures.snapshot(account: "b", relay: "second"))
    await backend.setPage(page(["new"], generation: 2))
    await store.reload(refreshProjection: false)
    await pause.resume.open()
    await TeraScopeFixtures.eventually { store.cards.map(\.id) == ["new"] }
    XCTAssertNil(store.presentation.readFailure)
    store.stop()
    _ = try await client.stop()
  }

  func testMandatoryResnapshotFailureRemovesUnqualifiedContent() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    let pause = await backend.pause(.reconcile, fails: true)
    await backend.emit(.initial, delivery: .resnapshotRequired)
    await pause.entered.wait()
    await pause.resume.open()
    await TeraScopeFixtures.eventually { store.hasPendingContent }
    XCTAssertTrue(store.cards.isEmpty)
    XCTAssertNil(store.currentCard(id: "one"))
    XCTAssertFalse(store.canLoadNextPage)
    XCTAssertNotNil(store.presentation.readFailure)
    store.stop()
    _ = try await client.stop()
  }

  func testVisibilityUpdateSupersedesAnOlderPageAlreadyInFlight() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    let pause = await backend.pause(.page)
    let paging = Task { await store.loadNextPage() }
    await pause.entered.wait()
    await backend.setReconciliation([TeraScopeFixtures.card("one")], generation: 2)
    await backend.emit(.today)
    await TeraScopeFixtures.eventually { store.hasPendingContent }
    await pause.resume.open()
    await paging.value
    XCTAssertEqual(store.cards.map(\.id), ["one"])
    XCTAssertNil(store.currentCard(id: "two"))
    XCTAssertFalse(store.canLoadNextPage)
    store.stop()
    _ = try await client.stop()
  }

  func testReconciliationDoesNotCombineChangedGenerationsAcrossBatches() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let cards = (0 ..< 101).map { TeraScopeFixtures.card(String($0)) }
    await backend.setReconciliation(cards, generation: 1)
    let pause = await backend.pause(.reconcile)
    let read = Task {
      try await TeraTodayReconciler.read(
        client: client, context: TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot()),
        asOf: 1, cards: cards
      )
    }
    await pause.entered.wait()
    await backend.setReconciliation(cards, generation: 2)
    await pause.resume.open()
    do {
      _ = try await read.value
      XCTFail("A mixed projection cannot become the current visible snapshot")
    } catch {
      let requests = await backend.reconciliationRequests
      XCTAssertEqual(requests.map(\.cardIDs.count), [100, 1])
      XCTAssertEqual(requests.last?.expectedGeneration, 1)
    }
    _ = try await client.stop()
  }

  private func makeStore(_ backend: TeraScopeBackend) async throws -> (TeraRuntimeClient, TeraTodayStore) {
    await backend.setPage(page(["one", "two"], next: "next"))
    await backend.setPage(page(["three"], next: "last"), cursor: "next")
    await backend.setReconciliation(["one", "two", "three"].map(TeraScopeFixtures.card), generation: 1)
    let client = try await TeraScopeFixtures.client(backend)
    let initial = await backend.pause(.reconcile)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    await initial.entered.wait()
    await initial.resume.open()
    await TeraScopeFixtures.eventually { store.cards.count == 2 && !store.presentation.isReading }
    return (client, store)
  }

  private func page(_ ids: [String], next: String? = nil, generation: UInt64 = 1) -> TeraTodayPage {
    TeraTodayPage(
      asOfUnixSeconds: 1, items: ids.map(TeraScopeFixtures.card), nextCursor: next,
      projectionGeneration: generation
    )
  }
}
