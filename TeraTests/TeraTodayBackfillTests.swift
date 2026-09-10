@testable import TeraApp
import XCTest

@MainActor
final class TeraTodayBackfillTests: XCTestCase {
  func testExplicitBackfillPreservesCacheAndIncompleteHistoryAcrossLocalReloads() async throws {
    let session = try await prepared()
    let backend = session.backend
    let client = session.client
    let store = session.store
    let cached = store.cards
    await store.reload(refreshProjection: false)
    XCTAssertEqual(store.discovery.continuation, "older")
    let initialCalls = await backend.counts[.refresh]
    XCTAssertEqual(initialCalls, 1)

    await backend.setSyncReceipt(receipt(nil, incomplete: true))
    let pause = await backend.pause(.refresh)
    let search = Task { await store.searchOlderPosts() }
    await pause.entered.wait()
    XCTAssertTrue(store.discovery.isSearching)
    XCTAssertFalse(store.discovery.canSearchOlder)
    XCTAssertEqual(store.cards, cached)
    await store.searchOlderPosts()
    await store.reload(refreshProjection: false)
    XCTAssertTrue(store.discovery.isSearching)
    XCTAssertEqual(store.discovery.continuation, "older")
    let cursor = await backend.lastBackfillCursor
    let calls = await backend.counts[.refresh]
    XCTAssertEqual(cursor, "older")
    XCTAssertEqual(calls, 2)
    await pause.resume.open()
    await search.value
    XCTAssertFalse(store.discovery.isSearching)
    XCTAssertNil(store.discovery.continuation)
    XCTAssertTrue(store.discovery.hadIncompleteResponses)
    XCTAssertEqual(store.discovery.message, "Some relay responses were incomplete. Older searches may leave gaps.")
    XCTAssertEqual(store.presentation.relayReceipt, receipt(nil, incomplete: true))
    await backend.setSyncReceipt(receipt("new", incomplete: false))
    await store.reload()
    XCTAssertEqual(store.discovery.continuation, "new")
    XCTAssertFalse(store.discovery.hadIncompleteResponses)
    let ordinaryCursor = await backend.lastBackfillCursor
    XCTAssertNil(ordinaryCursor)
    _ = try await client.stop()
  }

  func testLateOldContextBackfillCannotRestoreContinuationOrCards() async throws {
    let session = try await prepared()
    let backend = session.backend
    let client = session.client
    let store = session.store
    await backend.setSyncReceipt(receipt("old-result", incomplete: true))
    let pause = await backend.pause(.refresh)
    let search = Task { await store.searchOlderPosts() }
    await pause.entered.wait()
    store.configure(snapshot: TeraScopeFixtures.snapshot(relay: "second"))
    XCTAssertNil(store.discovery.continuation)
    await backend.setSyncReceipt(receipt("new-context", incomplete: false))
    await store.reload()
    await pause.resume.open()
    await search.value
    XCTAssertEqual(store.discovery.continuation, "new-context")
    XCTAssertFalse(store.discovery.hadIncompleteResponses)
    XCTAssertEqual(store.cards.map(\.id), ["wss://second.example"])
    _ = try await client.stop()
  }

  func testCancelledBackfillKeepsRetryCursorAndIgnoresLateCompletion() async throws {
    let session = try await prepared()
    let backend = session.backend
    let client = session.client
    let store = session.store
    let cached = store.cards
    await backend.setSyncReceipt(receipt("late", incomplete: false))
    let pause = await backend.pause(.refresh)
    let search = Task { await store.searchOlderPosts() }
    await pause.entered.wait()
    search.cancel()
    await search.value
    XCTAssertFalse(store.discovery.isSearching)
    XCTAssertTrue(store.discovery.canSearchOlder)
    await pause.resume.open()
    XCTAssertEqual(store.discovery.continuation, "older")
    XCTAssertTrue(store.discovery.hadIncompleteResponses)
    XCTAssertEqual(store.cards, cached)
    _ = try await client.stop()
  }

  func testFailureKeepsRetryButStaleCursorRequiresNewSearch() async throws {
    let session = try await prepared()
    let backend = session.backend
    let client = session.client
    let store = session.store
    let cached = store.cards
    for stale in [false, true] {
      let failure: TeraRuntimeFailure = stale
        ? .local(operation: "test", code: "today_cursor_invalid", safeMessage: "Controlled stale search.")
        : TeraScopeFixtures.failure()
      let pause = await backend.pause(.refresh, failure: failure)
      let search = Task { await store.searchOlderPosts() }
      await pause.entered.wait()
      await pause.resume.open()
      await search.value
      XCTAssertEqual(store.discovery.failure?.requiresRefresh, stale)
      XCTAssertEqual(store.discovery.canSearchOlder, !stale)
      XCTAssertEqual(store.discovery.continuation, stale ? nil : "older")
      XCTAssertEqual(store.cards, cached)
    }
    _ = try await client.stop()
  }

  func testNewOrdinaryRefreshInvalidatesPendingOlderSearch() async throws {
    let session = try await prepared()
    let backend = session.backend
    let client = session.client
    let store = session.store
    await backend.setSyncReceipt(receipt("late", incomplete: true))
    let pause = await backend.pause(.refresh)
    let search = Task { await store.searchOlderPosts() }
    await pause.entered.wait()
    await backend.setSyncReceipt(receipt("new", incomplete: false))
    await store.reload()
    await pause.resume.open()
    await search.value
    XCTAssertEqual(store.discovery.continuation, "new")
    XCTAssertFalse(store.discovery.hadIncompleteResponses)
    XCTAssertFalse(store.discovery.isSearching)
    _ = try await client.stop()
  }

  private struct Session {
    let backend: TeraScopeBackend
    let client: TeraRuntimeClient
    let store: TeraTodayStore
  }

  private func prepared() async throws -> Session {
    let backend = try TeraScopeBackend()
    await backend.setSyncReceipt(receipt("older", incomplete: true))
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.reload()
    return Session(backend: backend, client: client, store: store)
  }

  private func receipt(_ cursor: String?, incomplete: Bool) -> TeraTodaySyncReceipt {
    TeraTodaySyncFixtures.receipt(
      discovery: TeraTodayDiscoveryReceipt(continuation: cursor, hadIncompleteResponses: incomplete)
    )
  }
}
