import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraVisibilityTests: XCTestCase {
  func testChangeClearsEveryVisibleStoreAndRejectsLateSearchAndMedia() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let stores = TeraProductStores(runtimeClient: client)
    stores.configure(snapshot: TeraScopeFixtures.snapshot())
    let context = try XCTUnwrap(stores.today.selectedContext)
    await stores.today.reload(refreshProjection: false)
    let card = try XCTUnwrap(stores.today.cards.first)
    stores.search.updateQuery("old")
    await stores.search.search()
    await stores.me.reload()
    XCTAssertFalse(stores.search.results.isEmpty)
    XCTAssertNotNil(stores.me.snapshot)
    let search = await backend.pause(.search)
    let searching = Task { await stores.search.search() }
    await search.entered.wait()
    let media = await backend.pause(.media)
    let reference = TeraScopeFixtures.reference()
    stores.media.load(media: reference, context: context)
    await media.entered.wait()
    let change = ResourceTestPause()
    await backend.visibilityStorage.hold(change)
    let oldScope = stores.today.scopeGeneration
    let changing = Task { await stores.visibility.change(author: card.authorPublicKey, to: .blocked) }
    await change.entered.wait()
    XCTAssertTrue(stores.today.cards.isEmpty)
    XCTAssertNil(stores.today.currentCard(id: card.id))
    XCTAssertNotEqual(stores.today.scopeGeneration, oldScope)
    XCTAssertTrue(stores.search.results.isEmpty)
    XCTAssertNil(stores.me.snapshot)
    await backend.setPage(TeraTodayPage(asOfUnixSeconds: 1, items: [], nextCursor: nil, calendar: TeraScopeFixtures.viewerCalendar(asOf: 1)))
    await change.resume.open()
    await changing.value
    await search.resume.open()
    await searching.value
    await media.resume.open()
    XCTAssertTrue(stores.search.results.isEmpty)
    XCTAssertTrue(stores.today.cards.isEmpty)
    XCTAssertNotEqual(stores.media.state(for: reference, context: context), try .ready(TeraScopeFixtures.artifact("a")))
    XCTAssertEqual(stores.visibility.policy?.entries.first?.visibility, .blocked)
    let counts = await backend.counts
    XCTAssertNil(counts[.refresh], "Visibility changes only re-read local data")
    XCTAssertEqual(counts[.media], 1)
    stores.stop()
    _ = try await client.stop()
  }

  func testUnconfirmedChangeRetainsClearedPresentation() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let stores = TeraProductStores(runtimeClient: client)
    stores.configure(snapshot: TeraScopeFixtures.snapshot())
    await stores.today.reload(refreshProjection: false)
    XCTAssertFalse(stores.today.cards.isEmpty)
    await backend.visibilityStorage.fail()
    await stores.visibility.change(author: String(repeating: "a", count: 64), to: .muted)
    XCTAssertTrue(stores.today.cards.isEmpty)
    XCTAssertNil(stores.visibility.policy)
    XCTAssertNotNil(stores.visibility.message)
    XCTAssertFalse(stores.visibility.isWorking)
    stores.stop()
    _ = try await client.stop()
  }

  func testRealGeneratedPolicyPersistsAndExplicitRestoreSurvivesRestart() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let client = TeraRuntimeClient.production()
    let signer = ComposerForbiddenSigner()
    let configuration = fixture.configuration(signer)
    let snapshot = try await client.start(configuration: configuration)
    let author = snapshot.identity.publicKeyHex
    let blocked = try await client.setAuthorVisibility(author: author, visibility: .blocked)
    XCTAssertEqual(blocked.entries, [TeraAuthorVisibilityEntry(author: author, visibility: .blocked)])
    XCTAssertFalse(String(reflecting: blocked).contains(author))
    XCTAssertFalse(String(reflecting: blocked.entries).contains(author))
    _ = try await client.stop()
    _ = try await client.start(configuration: configuration)
    let loaded = try await client.authorVisibility()
    XCTAssertEqual(loaded, blocked)
    let muted = try await client.setAuthorVisibility(author: author, visibility: .muted)
    XCTAssertEqual(muted.revision, blocked.revision + 1)
    let restored = try await client.setAuthorVisibility(author: author, visibility: .visible)
    XCTAssertTrue(restored.entries.isEmpty)
    XCTAssertEqual(restored.revision, muted.revision + 1)
    let signs = await signer.requests
    XCTAssertEqual(signs, 0)
    _ = try await client.stop()
  }
}

actor VisibilityTestStorage {
  private var policy = TeraAuthorVisibilityPolicy(revision: 0, entries: [])
  private var pause: ResourceTestPause?
  private var failed = false
  func hold(_ value: ResourceTestPause) {
    pause = value
  }

  func fail() {
    failed = true
  }

  func read() -> TeraAuthorVisibilityPolicy {
    policy
  }

  func change(author: String, to visibility: TeraAuthorVisibility) async throws -> TeraAuthorVisibilityPolicy {
    await pause?.wait()
    if failed {
      throw TeraComposerAcknowledgment.unconfirmed
    }
    policy = TeraAuthorVisibilityPolicy(revision: policy.revision + 1, entries: visibility == .visible ? [] : [.init(author: author, visibility: visibility)])
    return policy
  }
}

extension TeraScopeBackend {
  func authorVisibility() async -> TeraAuthorVisibilityPolicy {
    await visibilityStorage.read()
  }

  func setAuthorVisibility(author: String, visibility: TeraAuthorVisibility) async throws -> TeraAuthorVisibilityPolicy {
    try await visibilityStorage.change(author: author, to: visibility)
  }
}
