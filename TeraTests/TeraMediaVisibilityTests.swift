@testable import TeraApp
import XCTest

@MainActor
final class TeraMediaVisibilityTests: XCTestCase {
  func testRemovalCancelsMediaAndRefusesLateCompletionOrRetry() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraMediaStore(runtimeClient: client)
    let reference = TeraScopeFixtures.reference()
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let pause = await backend.pause(.media)
    store.load(media: reference, context: context)
    await pause.entered.wait()
    store.reconcileVisibility(previous: [reference], current: [], context: context)
    XCTAssertEqual(store.state(for: reference, context: context), .unavailable)
    store.retry(media: reference, context: context)
    store.load(media: reference, context: context)
    await pause.resume.open()
    let calls = await backend.counts[.media]
    XCTAssertEqual(calls, 1)
    XCTAssertEqual(store.state(for: reference, context: context), .unavailable)
    store.reset()
    _ = try await client.stop()
  }

  func testStillVisibleSharedReferenceRetainsItsVerifiedArtifact() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraMediaStore(runtimeClient: client)
    let reference = TeraScopeFixtures.reference()
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let expected = try TeraScopeFixtures.artifact("a")
    store.load(media: reference, context: context)
    await TeraScopeFixtures.eventually { store.state(for: reference, context: context) == .ready(expected) }
    store.reconcileVisibility(previous: [reference, reference], current: [reference], context: context)
    XCTAssertEqual(store.state(for: reference, context: context), .ready(expected))
    store.reset()
    _ = try await client.stop()
  }
}
