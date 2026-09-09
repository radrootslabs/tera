@testable import TeraApp
import XCTest

@MainActor
final class TeraTodayCacheFirstTests: XCTestCase {
  func testInitialLocalContentAppearsBeforeAStalledRefreshAndSurvivesCancellation() async throws {
    for starting in [true, false] {
      for cards in [[], [TeraScopeFixtures.card("saved")]] {
        let backend = try TeraScopeBackend()
        await backend.setPage(page(cards))
        let (client, store) = try await makeStore(backend)
        let refresh = await backend.pause(.refresh)
        let task = Task {
          if starting {
            await store.start()
          } else {
            await store.reload()
          }
        }
        await refresh.entered.wait()
        XCTAssertEqual(store.cards, cards)
        XCTAssertEqual(store.presentation.content, cards.isEmpty ? .empty : .available)
        XCTAssertEqual(store.presentation.refresh, .refreshing)
        XCTAssertEqual(store.presentation.freshness, .unconfirmed)
        XCTAssertFalse(store.presentation.isReading)
        let counts = await backend.counts
        XCTAssertEqual(counts[.page], 1)
        XCTAssertEqual(counts[.refresh], 1)
        task.cancel()
        await task.value
        XCTAssertEqual(store.cards, cards)
        XCTAssertEqual(store.presentation.refresh, .idle)
        store.stop()
        // Release only after the caller has completed without a relay result.
        await refresh.resume.open()
        _ = try await client.stop()
      }
    }
  }

  func testSuccessfulRefreshReplacesTheAlreadyVisibleLocalPage() async throws {
    let backend = try TeraScopeBackend()
    await backend.setPage(page([TeraScopeFixtures.card("saved")]))
    let (client, store) = try await makeStore(backend)
    let refresh = await backend.pause(.refresh)
    let task = Task { await store.reload() }
    await refresh.entered.wait()
    XCTAssertEqual(store.cards.map(\.id), ["saved"])
    await backend.setPage(page([TeraScopeFixtures.card("updated")]))
    let read = await backend.pause(.page)
    await refresh.resume.open()
    await read.entered.wait()
    XCTAssertEqual(store.cards.map(\.id), ["saved"])
    XCTAssertTrue(store.presentation.isReading)
    XCTAssertEqual(store.presentation.freshness, .unconfirmed)
    await read.resume.open()
    await task.value
    XCTAssertEqual(store.cards.map(\.id), ["updated"])
    XCTAssertEqual(store.presentation.freshness, .refreshed(contentGeneration: 1))
    _ = try await client.stop()
  }

  func testCancelingTheFirstLocalReadDoesNotStartRefreshOrClaimNetworkActivity() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    let read = await backend.pause(.page)
    let task = Task { await store.reload() }
    await read.entered.wait()
    XCTAssertEqual(store.presentation.refresh, .idle)
    XCTAssertTrue(store.presentation.isReading)
    XCTAssertEqual(store.presentation.content, .notLoaded)
    task.cancel()
    await task.value
    XCTAssertFalse(store.presentation.isReading)
    XCTAssertEqual(store.presentation.refresh, .idle)
    let counts = await backend.counts
    XCTAssertNil(counts[.refresh])
    await read.resume.open()
    _ = try await client.stop()
  }

  func testLateRefreshedPageCannotReplaceANewerLocalReload() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    let refresh = await backend.pause(.refresh)
    let old = Task { await store.reload() }
    await refresh.entered.wait()
    await backend.setPage(page([TeraScopeFixtures.card("obsolete")]))
    let oldPage = await backend.pause(.page)
    await refresh.resume.open()
    await oldPage.entered.wait()
    await backend.setPage(page([TeraScopeFixtures.card("current")]))
    await store.reload(refreshProjection: false)
    await oldPage.resume.open()
    await old.value
    XCTAssertEqual(store.cards.map(\.id), ["current"])
    XCTAssertEqual(store.presentation.freshness, .unconfirmed)
    XCTAssertFalse(store.presentation.isReading)
    _ = try await client.stop()
  }

  func testLateCachedPaginationCannotAppendToTheRefreshedPageAtTheSameTime() async throws {
    let backend = try TeraScopeBackend()
    await backend.setPage(page([TeraScopeFixtures.card("saved")], next: "old-next"))
    await backend.setPage(page([TeraScopeFixtures.card("old-next")]), cursor: "old-next")
    let (client, store) = try await makeStore(backend)
    let refresh = await backend.pause(.refresh)
    let reload = Task { await store.reload() }
    await refresh.entered.wait()
    XCTAssertTrue(store.canLoadNextPage)
    let oldPage = await backend.pause(.page)
    let pagination = Task { await store.loadNextPage() }
    await oldPage.entered.wait()
    await backend.setPage(page([TeraScopeFixtures.card("updated")], next: "new-next"))
    await backend.setPage(page([TeraScopeFixtures.card("new-next")]), cursor: "new-next")
    await refresh.resume.open()
    await reload.value
    XCTAssertEqual(store.cards.map(\.id), ["updated"])
    XCTAssertFalse(store.isLoadingNextPage)
    await oldPage.resume.open()
    await pagination.value
    XCTAssertEqual(store.cards.map(\.id), ["updated"])
    await store.loadNextPage()
    XCTAssertEqual(store.cards.map(\.id), ["updated", "new-next"])
    _ = try await client.stop()
  }

  func testScopeChangeDuringRefreshClearsCacheAndRejectsLateSuccess() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    let refresh = await backend.pause(.refresh)
    let old = Task { await store.reload() }
    await refresh.entered.wait()
    XCTAssertEqual(store.cards.map(\.id), ["wss://first.example"])
    store.configure(snapshot: TeraScopeFixtures.snapshot(relay: "second"))
    XCTAssertTrue(store.cards.isEmpty)
    XCTAssertEqual(store.presentation.content, .notLoaded)
    await store.reload(refreshProjection: false)
    let counts = await backend.counts
    await refresh.resume.open()
    await old.value
    XCTAssertEqual(store.cards.map(\.id), ["wss://second.example"])
    XCTAssertEqual(store.presentation.freshness, .unconfirmed)
    let current = await backend.counts
    XCTAssertEqual(current[.page], counts[.page])
    _ = try await client.stop()
  }

  private func makeStore(_ backend: TeraScopeBackend) async throws -> (TeraRuntimeClient, TeraTodayStore) {
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    return (client, store)
  }

  private func page(_ cards: [TeraTodayCard], next: String? = nil) -> TeraTodayPage {
    TeraTodayPage(asOfUnixSeconds: 1, items: cards, nextCursor: next)
  }
}
