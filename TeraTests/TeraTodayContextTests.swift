import Combine
@testable import TeraApp
import XCTest

@MainActor
final class TeraTodayContextTests: XCTestCase {
  func testReplacementPreservesOnlyPresentSelectionAndIgnoresUnknownChoice() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let first = context("first"), second = context("second")
    let store = TeraTodayStore(runtimeClient: client, contexts: [first, second], selectedContextID: second.id)
    await store.reload(refreshProjection: false)
    let original = store.scopeGeneration
    store.replaceContexts([first, second, second], selectedID: nil)
    store.selectContext(id: "absent")
    XCTAssertEqual(store.scopeGeneration, original)
    XCTAssertEqual(store.cards.map(\.id), second.relayURLs)
    store.replaceContexts([second, first], selectedID: "absent")
    XCTAssertEqual(store.selectedContext, second)
    store.replaceContexts([first], selectedID: second.id)
    XCTAssertEqual(store.selectedContext, first)
    XCTAssertTrue(store.cards.isEmpty)
    store.replaceContexts([], selectedID: nil)
    XCTAssertNil(store.selectedContext)
    XCTAssertFalse(store.canLoadNextPage)
    store.stop()
    _ = try await client.stop()
  }

  func testFirstConfigurationReconcilesInjectedChoicesWithRuntimeSnapshot() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let stale = context("old")
    let store = TeraTodayStore(runtimeClient: client, contexts: [stale])
    await store.reload(refreshProjection: false)
    let snapshot = TeraScopeFixtures.snapshot()
    store.configure(snapshot: snapshot)
    XCTAssertTrue(store.cards.isEmpty)
    XCTAssertEqual(store.contexts, [.defaultContext(snapshot: snapshot)])
    XCTAssertEqual(store.selectedContext?.id, "default")
    store.stop()
    _ = try await client.stop()
  }

  func testAccountAndProfileChangesInvalidateSameNamedScopeButEvidenceDoesNot() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.reload(refreshProjection: false)
    var generation = store.scopeGeneration
    store.configure(snapshot: TeraScopeFixtures.snapshot(evidence: TeraScopeFixtures.evidence(observedAt: 9)))
    XCTAssertEqual(store.scopeGeneration, generation)
    XCTAssertFalse(store.cards.isEmpty)
    for snapshot in [TeraScopeFixtures.snapshot(profile: "public"), TeraScopeFixtures.snapshot(account: "b", profile: "public")] {
      let paused = await backend.pause(.page)
      let old = Task { await store.reload(refreshProjection: false) }
      await paused.entered.wait()
      store.configure(snapshot: snapshot)
      XCTAssertNotEqual(store.scopeGeneration, generation)
      XCTAssertTrue(store.cards.isEmpty)
      XCTAssertEqual(store.selectedContext?.id, "default")
      await paused.resume.open()
      await old.value
      XCTAssertTrue(store.cards.isEmpty)
      generation = store.scopeGeneration
      await store.reload(refreshProjection: false)
      XCTAssertEqual(store.scopeGeneration, generation)
    }
    store.stop()
    _ = try await client.stop()
  }

  func testFirstPageSwitchClearsBeforeLabelForCachedAndEmptyDestinations() async throws {
    for cached in [false, true] {
      try await assertSwitch(nextPage: false, cached: cached)
    }
  }

  func testSameIDReplacementClearsBeforePublishingChangedLabelAndDefinition() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let original = context("first")
    let store = TeraTodayStore(runtimeClient: client, contexts: [original])
    await store.reload(refreshProjection: false)
    let paused = await backend.pause(.page)
    let old = Task { await store.reload(refreshProjection: false) }
    await paused.entered.wait()
    let updated = TeraLocalNetwork(
      schemaVersion: 1, id: original.id, label: "Updated network", relayURLs: ["wss://second.example"],
      locality: nil, followedAuthors: [], generation: 2
    )
    var published = false
    let subscription = store.$contexts.dropFirst().sink { values in
      published = true
      XCTAssertEqual(values, [updated])
      XCTAssertTrue(store.cards.isEmpty)
      XCTAssertEqual(store.presentation.content, .notLoaded)
    }
    store.replaceContexts([updated], selectedID: nil)
    XCTAssertTrue(published)
    XCTAssertEqual(store.selectedContext, updated)
    await TeraScopeFixtures.eventually { store.cards.map(\.id) == updated.relayURLs }
    await paused.resume.open()
    await old.value
    XCTAssertEqual(store.cards.map(\.id), updated.relayURLs)
    subscription.cancel()
    store.stop()
    _ = try await client.stop()
  }

  func testNextPageSwitchClearsBeforeLabelForCachedAndEmptyDestinations() async throws {
    for cached in [false, true] {
      try await assertSwitch(nextPage: true, cached: cached)
    }
  }

  func testContextSwitchClearsSupportingStoresBeforeLabelAndRejectsLateResults() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let stores = TeraProductStores(runtimeClient: client)
    stores.configure(snapshot: TeraScopeFixtures.snapshot())
    stores.search.updateQuery("old")
    await stores.search.search()
    await stores.me.reload()
    XCTAssertFalse(stores.search.results.isEmpty)
    XCTAssertNotNil(stores.me.snapshot)
    let searchPause = await backend.pause(.search), mePause = await backend.pause(.me)
    let search = Task { await stores.search.search() }, me = Task { await stores.me.reload() }
    await searchPause.entered.wait()
    await mePause.entered.wait()
    var published = false
    let subscription = stores.today.$selectedContextID.dropFirst().sink { selected in
      published = true
      XCTAssertEqual(selected, "second")
      XCTAssertTrue(stores.search.results.isEmpty)
      XCTAssertNil(stores.me.snapshot)
      XCTAssertEqual(stores.me.observationState, .stopped)
    }
    stores.today.replaceContexts([context("second")], selectedID: nil)
    XCTAssertTrue(published)
    await searchPause.resume.open()
    await mePause.resume.open()
    await search.value
    await me.value
    XCTAssertTrue(stores.search.results.isEmpty)
    XCTAssertNil(stores.me.snapshot)
    await stores.me.reload()
    let actual = await backend.lastMeContext
    XCTAssertEqual(actual, stores.today.selectedContext)
    subscription.cancel()
    stores.stop()
    _ = try await client.stop()
  }

  private func assertSwitch(nextPage: Bool, cached: Bool) async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let first = context("first"), second = context("second")
    let store = TeraTodayStore(runtimeClient: client, contexts: [first, second])
    await backend.setPage(page(["old"], cursor: "next"))
    await backend.setPage(page(["old-next"]), cursor: "next")
    await store.reload(refreshProjection: false)
    let paused = await backend.pause(.page)
    let old = Task {
      if nextPage {
        await store.loadNextPage()
      } else {
        await store.reload(refreshProjection: false)
      }
    }
    await paused.entered.wait()
    let expected = cached ? ["new"] : []
    await backend.setPage(page(expected))
    var changes = 0
    let subscription = store.$selectedContextID.dropFirst().sink { selected in
      changes += 1
      XCTAssertEqual(selected, second.id)
      XCTAssertTrue(store.cards.isEmpty)
      XCTAssertEqual(store.presentation.content, .notLoaded)
      XCTAssertFalse(store.isLoadingNextPage)
      XCTAssertFalse(store.canLoadNextPage)
    }
    let generation = store.scopeGeneration
    store.selectContext(id: second.id)
    XCTAssertNotEqual(store.scopeGeneration, generation)
    XCTAssertEqual(changes, 1)
    XCTAssertEqual(store.selectedContext?.label, "second")
    await TeraScopeFixtures.eventually { store.presentation.content == (cached ? .available : .empty) }
    await paused.resume.open()
    await old.value
    XCTAssertEqual(store.cards.map(\.id), expected)
    XCTAssertEqual(store.selectedContext, second)
    subscription.cancel()
    store.stop()
    _ = try await client.stop()
  }

  private func context(_ id: String) -> TeraLocalNetwork {
    TeraLocalNetwork(schemaVersion: 1, id: id, label: id, relayURLs: ["wss://\(id).example"],
                     locality: nil, followedAuthors: [], generation: 1)
  }

  private func page(_ ids: [String], cursor: String? = nil) -> TeraTodayPage {
    TeraTodayPage(asOfUnixSeconds: 1, items: ids.map(TeraScopeFixtures.card), nextCursor: cursor, calendar: TeraScopeFixtures.viewerCalendar(asOf: 1))
  }
}
