@testable import TeraApp
import XCTest

@MainActor
final class TeraTodayPresentationTests: XCTestCase {
  func testContentAvailabilityIsIndependentOfRunningSuccessfulAndFailedRefresh() async throws {
    for cached in [nil, [], [TeraScopeFixtures.card("cached")]] as [[TeraTodayCard]?] {
      for fails in [false, true] {
        try await assertRefresh(cached: cached, fails: fails)
      }
    }
  }

  func testReadFailureRetainsCachedContentAndDoesNotInventAnEmptyResult() async throws {
    for cached in [false, true] {
      let backend = try TeraScopeBackend()
      let (client, store) = try await makeStore(backend)
      if cached {
        await store.reload(refreshProjection: false)
      }
      let expected = store.presentation.content
      let firstRead = await backend.pause(.page, fails: true)
      let pause = await backend.pause(.page, fails: true)
      let task = Task { await store.reload() }
      await firstRead.entered.wait()
      await firstRead.resume.open()
      await pause.entered.wait()
      XCTAssertEqual(store.presentation.refresh, .completed)
      XCTAssertEqual(store.presentation.content, expected)
      XCTAssertTrue(store.presentation.isReading)
      XCTAssertEqual(store.presentation.freshness, .unconfirmed)
      await pause.resume.open()
      await task.value
      XCTAssertEqual(store.presentation.content, expected)
      XCTAssertNotNil(store.presentation.readFailure)
      XCTAssertEqual(store.presentation.refresh, .completed)
      XCTAssertEqual(store.presentation.freshness, .unconfirmed)
      XCTAssertFalse(store.presentation.isReading)
      XCTAssertTrue(store.presentation.accessibilityStatus.contains("Saved posts could not be read."))
      XCTAssertFalse(store.presentation.accessibilityStatus.contains("Refresh failed."))
      XCTAssertEqual(store.cards.isEmpty, !cached)
      _ = try await client.stop()
    }
  }

  func testCanceledRefreshStopsActivityWithoutDiscardingCachedContent() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    await store.reload(refreshProjection: false)
    let cards = store.cards
    let pause = await backend.pause(.refresh)
    let task = Task { await store.reload() }
    await pause.entered.wait()
    task.cancel()
    await task.value
    XCTAssertEqual(store.cards, cards)
    XCTAssertEqual(store.presentation.content, .available)
    XCTAssertEqual(store.presentation.refresh, .idle)
    XCTAssertEqual(store.presentation.freshness, .unconfirmed)
    XCTAssertFalse(store.presentation.isReading)
    XCTAssertFalse(store.presentation.accessibilityStatus.contains("Checking for updates."))
    await pause.resume.open()
    _ = try await client.stop()
  }

  func testOldReadCancellationCannotClearTheNewRefreshActivity() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    await store.reload(refreshProjection: false)
    let reading = await backend.pause(.page)
    let old = Task { await store.reload(refreshProjection: false) }
    await reading.entered.wait()
    let refreshing = await backend.pause(.refresh)
    let current = Task { await store.reload() }
    await refreshing.entered.wait()
    old.cancel()
    await old.value
    XCTAssertEqual(store.presentation.refresh, .refreshing)
    XCTAssertEqual(store.presentation.content, .available)
    XCTAssertFalse(store.presentation.isReading)
    XCTAssertTrue(store.presentation.accessibilityStatus.contains("Checking for updates."))
    await reading.resume.open()
    await refreshing.resume.open()
    await current.value
    XCTAssertEqual(store.presentation.refresh, .completed)
    XCTAssertEqual(store.presentation.freshness, .refreshed(contentGeneration: 1))
    _ = try await client.stop()
  }

  func testMissingContextDoesNotClaimAnEmptyReadOrSuccessfulRefresh() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client)
    await store.reload()
    XCTAssertEqual(store.presentation.content, .notLoaded)
    XCTAssertEqual(store.presentation.refresh, .idle)
    XCTAssertEqual(store.presentation.freshness, .unconfirmed)
    XCTAssertNotNil(store.presentation.readFailure)
    let counts = await backend.counts
    XCTAssertNil(counts[.refresh])
    XCTAssertNil(counts[.page])
    _ = try await client.stop()
  }

  func testPaginationReadFailureAndRecoveryPreserveTheRefreshOutcome() async throws {
    let backend = try TeraScopeBackend()
    await backend.setPage(page([TeraScopeFixtures.card("one")], next: "two"))
    await backend.setPage(page([TeraScopeFixtures.card("two")]), cursor: "two")
    let (client, store) = try await makeStore(backend)
    await store.reload()
    let pause = await backend.pause(.page, fails: true)
    let task = Task { await store.loadNextPage() }
    await pause.entered.wait()
    await pause.resume.open()
    await task.value
    XCTAssertEqual(store.cards.map(\.id), ["one"])
    XCTAssertEqual(store.presentation.content, .available)
    XCTAssertEqual(store.presentation.refresh, .completed)
    XCTAssertNotNil(store.presentation.readFailure)
    XCTAssertEqual(store.presentation.freshness, .unconfirmed)
    XCTAssertTrue(store.canLoadNextPage)
    XCTAssertFalse(store.presentation.readFailure?.requiresRefresh == true)
    await store.loadNextPage()
    XCTAssertEqual(store.cards.map(\.id), ["one", "two"])
    XCTAssertNil(store.presentation.readFailure)
    XCTAssertEqual(store.presentation.refresh, .completed)
    _ = try await client.stop()
  }

  func testFailedRefreshAndFailedLocalReadHaveIndependentAccessibleOutcomes() async throws {
    let backend = try TeraScopeBackend()
    let (client, store) = try await makeStore(backend)
    let refreshing = await backend.pause(.refresh, fails: true)
    let reading = await backend.pause(.page, fails: true)
    let rereading = await backend.pause(.page, fails: true)
    let task = Task { await store.reload() }
    await reading.entered.wait()
    await reading.resume.open()
    await refreshing.entered.wait()
    await refreshing.resume.open()
    await rereading.entered.wait()
    XCTAssertTrue(store.presentation.isReading)
    await rereading.resume.open()
    await task.value
    XCTAssertEqual(store.presentation.content, .notLoaded)
    XCTAssertNotNil(store.presentation.readFailure)
    XCTAssertTrue(store.presentation.accessibilityStatus.contains("Refresh failed."))
    XCTAssertTrue(store.presentation.accessibilityStatus.contains("Saved posts could not be read."))
    XCTAssertFalse(store.presentation.accessibilityStatus.contains("Reading saved posts."))
    _ = try await client.stop()
  }

  private func assertRefresh(cached: [TeraTodayCard]?, fails: Bool) async throws {
    let backend = try TeraScopeBackend()
    await backend.setPage(page(cached ?? []))
    let (client, store) = try await makeStore(backend)
    if cached != nil {
      await store.reload(refreshProjection: false)
    }
    let expected: TeraTodayContentAvailability = (cached ?? []).isEmpty ? .empty : .available
    let pause = await backend.pause(.refresh, fails: fails)
    let task = Task { await store.reload() }
    await pause.entered.wait()
    XCTAssertEqual(store.presentation.content, expected)
    XCTAssertEqual(store.presentation.refresh, .refreshing)
    XCTAssertEqual(store.presentation.freshness, .unconfirmed)
    XCTAssertFalse(store.presentation.isReading)
    XCTAssertTrue(store.presentation.accessibilityStatus.contains("Checking for updates."))
    XCTAssertEqual(store.cards, cached ?? [])
    await pause.resume.open()
    await task.value
    XCTAssertEqual(store.presentation.content, (cached ?? []).isEmpty ? .empty : .available)
    XCTAssertNil(store.presentation.readFailure)
    XCTAssertFalse(store.presentation.isReading)
    if fails {
      XCTAssertEqual(store.presentation.refresh, .failed(TeraTodayFailure(TeraScopeFixtures.failure())))
      XCTAssertEqual(store.presentation.freshness, .unconfirmed)
      XCTAssertTrue(store.presentation.accessibilityStatus.contains("Refresh failed."))
      XCTAssertTrue(store.presentation.accessibilityStatus.contains("Showing saved posts."))
    } else {
      XCTAssertEqual(store.presentation.refresh, .completed)
      XCTAssertEqual(store.presentation.freshness, .refreshed(contentGeneration: 1))
      XCTAssertEqual(store.presentation.accessibilityStatus, "Saved posts checked.")
    }
    _ = try await client.stop()
  }

  private func makeStore(_ backend: TeraScopeBackend) async throws -> (TeraRuntimeClient, TeraTodayStore) {
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    return (client, store)
  }

  private func page(_ cards: [TeraTodayCard], next: String? = nil) -> TeraTodayPage {
    TeraTodayPage(asOfUnixSeconds: 1, items: cards, nextCursor: next, calendar: TeraScopeFixtures.viewerCalendar(asOf: 1))
  }
}
