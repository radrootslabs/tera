@testable import TeraApp
import XCTest

@MainActor
final class TeraSupportingFreshnessTests: XCTestCase {
  func testSearchMandatoryFailureClearsOpenDetailAndRevokesMedia() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let stores = TeraProductStores(runtimeClient: client)
    stores.configure(snapshot: TeraScopeFixtures.snapshot())
    let store = stores.search
    let reference = TeraScopeFixtures.reference()
    let profile = profile(reference)
    let result = TeraSearchResult(type: .profile, id: profile.authorPublicKey, card: nil, profile: profile)
    await backend.setSearchResults([result])
    store.updateQuery("old farm")
    store.start()
    await TeraScopeFixtures.eventually { store.state == .loaded }
    XCTAssertNotNil(store.currentResult(id: result.id, type: .profile, context: context))
    let media = await backend.pause(.media)
    stores.media.load(media: reference, context: context)
    await media.entered.wait()
    let failure = await backend.pause(.search, fails: true)
    await backend.emit(.today, context: context)
    await failure.entered.wait()
    XCTAssertEqual(store.query, "old farm")
    XCTAssertTrue(store.results.isEmpty)
    XCTAssertNil(store.currentResult(id: result.id, type: .profile, context: context))
    XCTAssertEqual(stores.media.state(for: reference, context: context), .unavailable)
    await media.resume.open()
    await failure.resume.open()
    await TeraScopeFixtures.eventually {
      if case .failed = store.state {
        true
      } else {
        false
      }
    }
    XCTAssertTrue(store.results.isEmpty)
    XCTAssertNil(stores.media.image(for: reference, context: context))
    XCTAssertEqual(stores.media.cachedByteCount, 0)
    stores.stop()
    _ = try await client.stop()
  }

  func testSearchPausedOldResultCannotReturnAfterReplacementInvalidation() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let store = TeraSearchStore(runtimeClient: client)
    store.configure(context: context)
    store.updateQuery("scope")
    await backend.setSearchResults([result("old")])
    store.start()
    await TeraScopeFixtures.eventually { store.results.first?.id == "old" }
    let first = await backend.pause(.search)
    await backend.emit(.today, context: context)
    await first.entered.wait()
    let current = await backend.pause(.search)
    await backend.setSearchResults([result("current")])
    await backend.emit(.profile, context: context)
    XCTAssertTrue(store.results.isEmpty)
    await first.resume.open()
    await current.entered.wait()
    XCTAssertTrue(store.results.isEmpty, "The completed old read must remain fenced")
    await current.resume.open()
    await TeraScopeFixtures.eventually { store.results.first?.id == "current" }
    XCTAssertNil(store.currentResult(id: "old", type: .card, context: context))
    XCTAssertNotNil(store.currentResult(id: "current", type: .card, context: context))
    let calls = await backend.counts[.search]
    XCTAssertEqual(calls, 3, "Initial read and one bounded worker with one pending reconciliation")
    store.stop()
    _ = try await client.stop()
  }

  func testMeVoluntaryOfflineFailureKeepsCacheButMandatoryFailureClearsIt() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let store = TeraMeStore(runtimeClient: client)
    store.configure(context: context)
    await store.reload()
    XCTAssertNotNil(store.currentCard(id: "old", context: context))
    let voluntary = await backend.pause(.me, failure: .local(
      operation: "today", code: "today_storage_failed", safeMessage: "The local store is temporarily unavailable."
    ))
    let reload = Task { await store.reload() }
    await voluntary.entered.wait()
    await voluntary.resume.open()
    await reload.value
    XCTAssertNotNil(store.currentCard(id: "old", context: context))
    let mandatory = await backend.pause(.me, fails: true)
    store.invalidatePresentation()
    XCTAssertNil(store.snapshot)
    let reconcile = Task { await store.reload() }
    await mandatory.entered.wait()
    await mandatory.resume.open()
    await reconcile.value
    XCTAssertNil(store.snapshot)
    XCTAssertNil(store.currentCard(id: "old", context: context))
    store.stop()
    _ = try await client.stop()
  }

  func testMeObservationInvalidatesPausedReadAndGapReconcilesCurrentValues() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let store = TeraMeStore(runtimeClient: client)
    store.configure(context: context)
    await store.start()
    XCTAssertNotNil(store.snapshot, "Awaited startup includes the mandatory initial reconciliation")
    await TeraScopeFixtures.eventually { store.state == .loaded }
    let old = await backend.pause(.me)
    await backend.emit(.today, context: context)
    await old.entered.wait()
    let current = await backend.pause(.me)
    await backend.setMeCards([TeraScopeFixtures.card("current")])
    await backend.emit(.initial, delivery: .resnapshotRequired, context: context)
    XCTAssertNil(store.snapshot)
    await old.resume.open()
    await current.entered.wait()
    XCTAssertNil(store.snapshot)
    await current.resume.open()
    await TeraScopeFixtures.eventually { store.snapshot?.cards.first?.id == "current" }
    XCTAssertNil(store.currentCard(id: "old", context: context))
    store.stop()
    _ = try await client.stop()
  }

  func testFullContextChangeAndStopFenceSupportingReads() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let first = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let second = TeraLocalNetwork(
      schemaVersion: first.schemaVersion, id: first.id, label: first.label,
      relayURLs: first.relayURLs, locality: "Another locality",
      followedAuthors: first.followedAuthors, generation: first.generation
    )
    let search = TeraSearchStore(runtimeClient: client)
    let me = TeraMeStore(runtimeClient: client)
    search.configure(context: first)
    search.updateQuery("old")
    me.configure(context: first)
    let searchPause = await backend.pause(.search)
    let mePause = await backend.pause(.me)
    let searchTask = Task { await search.search() }
    let meTask = Task { await me.reload() }
    await searchPause.entered.wait()
    await mePause.entered.wait()
    search.configure(context: second)
    me.configure(context: second)
    await searchPause.resume.open()
    await mePause.resume.open()
    await searchTask.value
    await meTask.value
    XCTAssertTrue(search.results.isEmpty)
    XCTAssertNil(me.snapshot)
    search.updateQuery("new")
    await search.search()
    await me.reload()
    XCTAssertNil(search.currentResult(id: "new", type: .card, context: first))
    XCTAssertNotNil(search.currentResult(id: "new", type: .card, context: second))
    XCTAssertNil(me.currentCard(id: "old", context: first))
    XCTAssertNotNil(me.currentCard(id: "old", context: second))
    search.stop()
    me.stop()
    XCTAssertTrue(search.results.isEmpty)
    XCTAssertNil(me.snapshot)
    _ = try await client.stop()
  }

  func testMeMandatoryObserverFailureRemovesCurrentDetail() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let store = TeraMeStore(runtimeClient: client)
    store.configure(context: context)
    await store.start()
    await TeraScopeFixtures.eventually { store.state == .loaded }
    XCTAssertNotNil(store.currentCard(id: "old", context: context))
    let failed = await backend.pause(.me, fails: true)
    await backend.emit(.today, context: context)
    await failed.entered.wait()
    XCTAssertNil(store.currentCard(id: "old", context: context))
    await failed.resume.open()
    await TeraScopeFixtures.eventually {
      if case .failed = store.state {
        true
      } else {
        false
      }
    }
    XCTAssertNil(store.snapshot)
    store.stop()
    _ = try await client.stop()
  }

  func testOrdinaryMediaProgressPreservesCurrentSupportingContent() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let search = TeraSearchStore(runtimeClient: client)
    let me = TeraMeStore(runtimeClient: client)
    search.configure(context: context)
    search.updateQuery("scope")
    search.start()
    me.configure(context: context)
    await me.start()
    await TeraScopeFixtures.eventually { search.state == .loaded && me.state == .loaded }
    let results = search.results
    let snapshot = me.snapshot
    let before = await backend.counts
    for _ in 1 ... 8 {
      await backend.emit(.media, context: context)
      try await Task.sleep(for: .milliseconds(10))
    }
    let after = await backend.counts
    XCTAssertEqual(after[.search], before[.search])
    XCTAssertEqual(after[.me], before[.me])
    XCTAssertEqual(search.results, results)
    XCTAssertEqual(me.snapshot, snapshot)
    search.stop()
    me.stop()
    _ = try await client.stop()
  }

  private func result(_ id: String) -> TeraSearchResult {
    TeraSearchResult(type: .card, id: id, card: TeraScopeFixtures.card(id), profile: nil)
  }

  private func profile(_ reference: TeraMediaReference) -> TeraProfileSummary {
    TeraProfileSummary(
      authorPublicKey: String(repeating: "a", count: 64), name: "Farm", displayName: nil,
      about: nil, picture: reference, banner: nil, nip05: nil, website: nil, lightningAddress: nil
    )
  }
}

extension TeraSupportingFreshnessTests {
  func testMeTypedCurrentScopeRefusalClearsCacheWithoutNotification() async throws {
    let codes = [
      "today_invalid_request", "today_refresh_required", "today_projection_rebuilding",
      "today_cursor_invalid", "today_reader_unsupported", "today_runtime_unavailable", "today_state_failed",
      "storage_space_insufficient",
    ]
    for code in codes {
      let backend = try TeraScopeBackend()
      let client = try await TeraScopeFixtures.client(backend)
      let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
      let stores = TeraProductStores(runtimeClient: client)
      stores.configure(snapshot: TeraScopeFixtures.snapshot())
      let store = stores.me
      let reference = TeraScopeFixtures.reference()
      await backend.setMeProfile(profile(reference))
      await store.reload()
      XCTAssertNotNil(store.currentCard(id: "old", context: context), code)
      XCTAssertNotNil(store.currentProfile(context: context), code)
      stores.media.load(media: reference, context: context)
      await TeraScopeFixtures.eventually { stores.media.image(for: reference, context: context) != nil }
      XCTAssertGreaterThan(stores.media.cachedByteCount, 0, code)
      let pause = await backend.pause(.me, failure: .local(
        operation: "today", code: code, safeMessage: "The current content could not be confirmed."
      ))
      let reload = Task { await store.reload() }
      await pause.entered.wait()
      await pause.resume.open()
      await reload.value
      XCTAssertNil(store.snapshot, code)
      XCTAssertNil(store.currentCard(id: "old", context: context), code)
      XCTAssertNil(store.currentProfile(context: context), code)
      XCTAssertEqual(stores.media.state(for: reference, context: context), .unavailable, code)
      XCTAssertNil(stores.media.image(for: reference, context: context), code)
      XCTAssertEqual(stores.media.cachedByteCount, 0, code)
      if case .failed = store.state {} else {
        XCTFail("Expected a typed read refusal for \(code)")
      }
      stores.stop()
      _ = try await client.stop()
    }
  }
}

extension TeraSupportingFreshnessTests {
  func testSearchQueryChangeAndStopPreserveMeSharedMediaUntilLastCurrentReferenceLeaves() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let stores = TeraProductStores(runtimeClient: client)
    stores.configure(snapshot: TeraScopeFixtures.snapshot())
    let reference = TeraScopeFixtures.reference()
    let shared = profile(reference)
    let result = TeraSearchResult(type: .profile, id: shared.authorPublicKey, card: nil, profile: shared)
    await backend.setMeProfile(shared)
    await stores.me.reload()
    await backend.setSearchResults([result])
    stores.search.updateQuery("farm")
    await stores.search.search()
    XCTAssertNotNil(stores.me.currentProfile(context: context))
    XCTAssertNotNil(stores.search.currentResult(id: result.id, type: .profile, context: context))
    stores.media.load(media: reference, context: context)
    await TeraScopeFixtures.eventually { stores.media.image(for: reference, context: context) != nil }
    let expected = try TeraScopeFixtures.artifact("a")
    XCTAssertEqual(stores.media.state(for: reference, context: context), .ready(expected))
    stores.search.updateQuery("different query")
    XCTAssertTrue(stores.search.results.isEmpty)
    XCTAssertNil(stores.search.currentResult(id: result.id, type: .profile, context: context))
    XCTAssertEqual(stores.me.currentProfile(context: context), shared)
    XCTAssertNotNil(stores.media.image(for: reference, context: context))
    XCTAssertEqual(stores.media.state(for: reference, context: context), .ready(expected))
    stores.media.load(media: reference, context: context)
    stores.media.retry(media: reference, context: context)
    await TeraScopeFixtures.eventually { stores.media.state(for: reference, context: context) == .ready(expected) }
    let calls = await backend.counts[.media]
    XCTAssertEqual(calls, 2, "Retry remains usable for the unchanged current Me picture")
    await stores.search.search()
    stores.search.stop()
    XCTAssertEqual(stores.me.currentProfile(context: context), shared)
    XCTAssertNotNil(stores.media.image(for: reference, context: context))
    stores.me.invalidatePresentation()
    XCTAssertNil(stores.me.currentProfile(context: context))
    XCTAssertNil(stores.media.image(for: reference, context: context))
    XCTAssertEqual(stores.media.cachedByteCount, 0)
    stores.media.load(media: reference, context: context)
    stores.media.retry(media: reference, context: context)
    let finalCalls = await backend.counts[.media]
    XCTAssertEqual(finalCalls, 2, "Last-reference invalidation must still deny load and retry")
    stores.stop()
    _ = try await client.stop()
  }
}
