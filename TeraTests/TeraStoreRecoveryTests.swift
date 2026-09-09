@testable import TeraApp
import XCTest

@MainActor
final class TeraStoreRecoveryTests: XCTestCase {
  func testRecoveredTodayAddAndMeReadCurrentStateWithoutAnotherEvent() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let retry = ResourceTestPause()
    let today = TeraTodayStore(runtimeClient: client, observationDelay: { _ in await retry.wait() })
    let add = TeraAddStore(runtimeClient: client, observationDelay: { _ in await retry.wait() })
    let me = TeraMeStore(runtimeClient: client, observationDelay: { _ in await retry.wait() })
    configure(today: today, add: add, me: me)
    await today.start()
    await add.start()
    await me.start()
    await TeraScopeFixtures.eventually {
      today.observationState == .active && add.observationState == .active && me.observationState == .active
    }
    await client.suspend()
    await TeraScopeFixtures.eventually {
      self.isRetrying(today.observationState) && self.isRetrying(add.observationState) && self.isRetrying(me.observationState)
    }
    await setCurrentState(backend, revision: 2)
    await retry.resume.open()
    await TeraScopeFixtures.eventually {
      today.cards.first?.id == "revision-2" && add.drafts.first?.revision == 2
        && me.snapshot?.cards.first?.id == "revision-2" && add.blossomEvidence?.observedAtUnixMilliseconds == 2
    }
    let subscriptions = await backend.counts[.subscribe]
    XCTAssertEqual(subscriptions, 6)
    today.stop()
    add.stop()
    me.stop()
    _ = try await client.stop()
  }

  func testFinalGapRefreshesAllStoreDomainsWithoutOverwritingTheEditingForm() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let today = TeraTodayStore(runtimeClient: client)
    let add = TeraAddStore(runtimeClient: client)
    let me = TeraMeStore(runtimeClient: client)
    configure(today: today, add: add, me: me)
    await today.start()
    await add.start()
    await me.start()
    await TeraScopeFixtures.eventually {
      today.observationState == .active && add.observationState == .active && me.observationState == .active
    }
    add.updateForm(\.content, "Unsaved local editing")
    await setCurrentState(backend, revision: 9)
    await backend.emit(.initial, delivery: .resnapshotRequired)
    await TeraScopeFixtures.eventually {
      today.cards.first?.id == "revision-9" && add.drafts.first?.revision == 9
        && me.snapshot?.cards.first?.id == "revision-9" && add.blossomEvidence?.observedAtUnixMilliseconds == 9
    }
    XCTAssertEqual(add.form.content, "Unsaved local editing")
    let refreshes = await backend.counts[.refresh]
    XCTAssertEqual(refreshes, 1, "Observation recovery reads local state without another network refresh")
    today.stop()
    add.stop()
    me.stop()
    _ = try await client.stop()
  }

  func testTodayStormDuringSlowReadEventuallyDisplaysTheFinalDurablePage() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    await TeraScopeFixtures.eventually { store.observationState == .active && !store.cards.isEmpty }
    let first = await backend.pause(.page)
    await backend.emit(.today)
    await first.entered.wait()
    let before = await backend.counts[.page, default: 0]
    await setCurrentState(backend, revision: 256)
    for _ in 1 ... 256 {
      await backend.emit(.today)
    }
    let paused = await backend.counts[.page, default: 0]
    XCTAssertEqual(paused, before, "Only one observer query may execute at a time")
    await first.resume.open()
    await TeraScopeFixtures.eventually { store.cards.first?.id == "revision-256" }
    XCTAssertEqual(store.state, .loaded)
    store.stop()
    _ = try await client.stop()
  }

  func testOrdinaryMediaProgressPreservesLoadedPagesAndTheNextCursor() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let subscription = await backend.pause(.subscribe)
    await backend.setPage(page("one", next: "two"))
    await backend.setPage(page("two", next: "three"), cursor: "two")
    await backend.setPage(page("three"), cursor: "three")
    let store = TeraTodayStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    await backend.setPage(page("current-one", next: "two"))
    await subscription.resume.open()
    await TeraScopeFixtures.eventually { store.cards.first?.id == "current-one" }
    await store.loadNextPage()
    XCTAssertEqual(store.cards.map(\.id), ["current-one", "two"])
    let before = await backend.counts[.page, default: 0]
    // Space hints beyond one actor turn so this exercises ordinary progress,
    // independently of C029's intentionally stronger overflow resnapshot.
    for _ in 1 ... 8 {
      await backend.emit(.media)
      try await Task.sleep(for: .milliseconds(10))
    }
    let after = await backend.counts[.page, default: 0]
    XCTAssertEqual(after, before)
    XCTAssertEqual(store.cards.map(\.id), ["current-one", "two"])
    XCTAssertTrue(store.canLoadNextPage)
    await store.loadNextPage()
    XCTAssertEqual(store.cards.map(\.id), ["current-one", "two", "three"])
    store.stop()
    _ = try await client.stop()
  }

  private func configure(today: TeraTodayStore, add: TeraAddStore, me: TeraMeStore) {
    let snapshot = TeraScopeFixtures.snapshot()
    today.configure(snapshot: snapshot)
    add.configure(snapshot: snapshot)
    me.configure(context: .defaultContext(snapshot: snapshot))
  }

  private func setCurrentState(_ backend: TeraScopeBackend, revision: UInt64) async {
    await backend.setPage(page("revision-\(revision)"))
    await backend.setDrafts([TeraScopeFixtures.draft("revision-\(revision)", revision: revision)])
    await backend.setMeCards([TeraScopeFixtures.card("revision-\(revision)")])
    await backend.configure(TeraScopeFixtures.snapshot(evidence: TeraScopeFixtures.evidence(observedAt: revision)))
  }

  private func page(_ id: String, next: String? = nil) -> TeraTodayPage {
    TeraTodayPage(asOfUnixSeconds: 1, items: [TeraScopeFixtures.card(id)], nextCursor: next)
  }

  private func isRetrying(_ value: TeraRuntimeObservationState) -> Bool {
    if case .retrying = value {
      return true
    }
    return false
  }
}
