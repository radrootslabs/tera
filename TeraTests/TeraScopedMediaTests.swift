@testable import TeraApp
import XCTest

@MainActor
final class TeraScopedMediaTests: XCTestCase {
  func testCancelledMediaOwnerCannotClearAReplacementTaskOrAdmitDuplicateWork() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraMediaStore(runtimeClient: client)
    let reference = TeraScopeFixtures.reference()
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let first = await backend.pause(.media)
    store.load(media: reference, context: context)
    await first.entered.wait()
    let second = await backend.pause(.media)
    let expected = try TeraScopeFixtures.artifact("b")
    await backend.setMedia(expected)
    store.retry(media: reference, context: context)
    await second.entered.wait()
    await first.resume.open()
    // A completed actor hop gives the cancelled owner a chance to finish while
    // the replacement is held at its explicit backend gate.
    await TeraScopeFixtures.eventually { store.state(for: reference, context: context) == .loading }
    store.load(media: reference, context: context)
    await second.resume.open()
    await TeraScopeFixtures.eventually { store.state(for: reference, context: context) == .ready(expected) }
    let calls = await backend.counts[.media]
    XCTAssertEqual(calls, 2)
    store.reset()
    _ = try await client.stop()
  }

  func testAccountResetCannotBeRepopulatedByLateMediaAndSameContextCanReload() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraMediaStore(runtimeClient: client)
    let reference = TeraScopeFixtures.reference()
    let first = TeraScopeFixtures.snapshot()
    let context = TeraLocalNetwork.defaultContext(snapshot: first)
    store.configure(snapshot: first)
    let pause = await backend.pause(.media)
    store.load(media: reference, context: context)
    await pause.entered.wait()
    store.configure(snapshot: TeraScopeFixtures.snapshot(account: "b"))
    await pause.resume.open()
    let current = try TeraScopeFixtures.artifact("b")
    await backend.setMedia(current)
    store.load(media: reference, context: context)
    await TeraScopeFixtures.eventually { store.state(for: reference, context: context) == .ready(current) }
    let calls = await backend.counts[.media]
    XCTAssertEqual(calls, 2)
    store.reset()
    _ = try await client.stop()
  }

  func testLateCorruptInvalidationCannotReplaceNewVerifiedMedia() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraMediaStore(runtimeClient: client)
    let reference = TeraScopeFixtures.reference()
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    try await backend.setMedia(TeraScopeFixtures.artifact("a", corrupt: true))
    let pause = await backend.pause(.invalidate)
    store.load(media: reference, context: context)
    await pause.entered.wait()
    let current = try TeraScopeFixtures.artifact("b")
    await backend.setMedia(current)
    store.retry(media: reference, context: context)
    await TeraScopeFixtures.eventually { store.state(for: reference, context: context) == .ready(current) }
    await pause.resume.open()
    store.load(media: reference, context: context)
    let calls = await backend.counts[.media]
    XCTAssertEqual(calls, 2)
    XCTAssertEqual(store.state(for: reference, context: context), .ready(current))
    store.reset()
    _ = try await client.stop()
  }
}
