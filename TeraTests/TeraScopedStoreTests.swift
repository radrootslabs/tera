@testable import TeraApp
import XCTest

@MainActor
final class TeraScopedStoreTests: XCTestCase {
  func testTodayReconfigurationClearsOldCardsSynchronouslyAndRejectsLatePages() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.reload(refreshProjection: false)
    XCTAssertEqual(store.cards.map(\.id), ["wss://first.example"])
    let pause = await backend.pause(.page)
    let old = Task { await store.reload(refreshProjection: false) }
    await pause.entered.wait()
    store.configure(snapshot: TeraScopeFixtures.snapshot(account: "b", relay: "second"))
    XCTAssertTrue(store.cards.isEmpty)
    XCTAssertEqual(store.selectedContext?.relayURLs, ["wss://second.example"])
    await store.reload(refreshProjection: false)
    await pause.resume.open()
    await old.value
    XCTAssertEqual(store.cards.map(\.id), ["wss://second.example"])
    XCTAssertEqual(store.presentation.content, .available)
    store.stop()
    _ = try await client.stop()
  }

  func testLateRefreshFailureDoesNotIssueAnOldScopePageOrChangeNewState() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    let pause = await backend.pause(.refresh, fails: true)
    let old = Task { await store.reload() }
    await pause.entered.wait()
    store.configure(snapshot: TeraScopeFixtures.snapshot(relay: "second"))
    await store.reload(refreshProjection: false)
    let count = await backend.counts[.page]
    await pause.resume.open()
    await old.value
    let after = await backend.counts[.page]
    XCTAssertEqual(after, count)
    XCTAssertEqual(store.cards.map(\.id), ["wss://second.example"])
    XCTAssertEqual(store.presentation.content, .available)
    store.stop()
    _ = try await client.stop()
  }

  func testSearchQueryEditsRejectLateSuccessAndFailureWithoutAnotherSearch() async throws {
    for fails in [false, true] {
      let backend = try TeraScopeBackend()
      let client = try await TeraScopeFixtures.client(backend)
      let store = TeraSearchStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
      store.configure(context: .defaultContext(snapshot: TeraScopeFixtures.snapshot()))
      store.updateQuery("old")
      let pause = await backend.pause(.search, fails: fails)
      let old = Task { await store.search() }
      await pause.entered.wait()
      store.updateQuery("new")
      await pause.resume.open()
      await old.value
      XCTAssertEqual(store.query, "new")
      XCTAssertTrue(store.results.isEmpty)
      XCTAssertEqual(store.state, .idle)
      _ = try await client.stop()
    }
  }

  func testAccountReconfigurationClearsSupportingStateAndRejectsLateMe() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let stores = TeraProductStores(runtimeClient: client)
    let first = TeraScopeFixtures.snapshot()
    stores.configure(snapshot: first)
    stores.me.configure(context: .defaultContext(snapshot: first))
    stores.search.configure(context: .defaultContext(snapshot: first))
    stores.search.updateQuery("old")
    await stores.search.search()
    let pause = await backend.pause(.me)
    let old = Task { await stores.me.reload() }
    await pause.entered.wait()
    stores.settings.profileName = "Old account"
    let updated = TeraScopeFixtures.snapshot(account: "b", relay: "second")
    await backend.configure(updated)
    stores.configure(snapshot: updated)
    XCTAssertTrue(stores.search.results.isEmpty)
    XCTAssertEqual(stores.settings.profileName, "")
    await pause.resume.open()
    await old.value
    XCTAssertNil(stores.me.snapshot)
    XCTAssertEqual(stores.me.state, .idle)
    await stores.me.reload()
    let requested = await backend.lastMeContext
    XCTAssertEqual(requested, stores.today.selectedContext)
    XCTAssertEqual(stores.me.snapshot?.publicKey, updated.identity.publicKeyHex)
    stores.stop()
    _ = try await client.stop()
  }

  func testStoppedProductResumeCannotInstallLateStartupResults() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let stores = TeraProductStores(runtimeClient: client)
    stores.configure(snapshot: TeraScopeFixtures.snapshot())
    let firstObserver = await backend.pause(.subscribe)
    let secondObserver = await backend.pause(.subscribe)
    let drafts = await backend.pause(.drafts)
    let pause = await backend.pause(.page)
    let old = Task { await stores.resume() }
    await pause.entered.wait()
    await drafts.entered.wait()
    let validated = stores.add.schemas
    XCTAssertEqual(validated.count, 5)
    stores.stop()
    await old.value
    XCTAssertTrue(stores.add.isProductReady)
    XCTAssertEqual(stores.add.schemas, validated)
    XCTAssertTrue(stores.add.drafts.isEmpty)
    XCTAssertEqual(stores.add.observationState, .stopped)
    let calls = await backend.counts[.drafts, default: 0]
    XCTAssertEqual(calls, 1)
    await pause.resume.open()
    await drafts.resume.open()
    await firstObserver.resume.open()
    await secondObserver.resume.open()
    _ = try await client.stop()
    XCTAssertEqual(stores.add.schemas, validated)
    XCTAssertTrue(stores.add.drafts.isEmpty)
  }
}
