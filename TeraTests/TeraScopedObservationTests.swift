@testable import TeraApp
import XCTest

@MainActor
final class TeraScopedObservationTests: XCTestCase {
  func testCancelledLateSubscriptionCannotClearOrUpdateReplacementObserver() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    let first = await backend.pause(.subscribe)
    await store.start()
    await first.entered.wait()
    store.stop()
    let second = await backend.pause(.subscribe)
    await store.start()
    await second.entered.wait()
    await first.resume.open()
    let tokens = await backend.tokens
    await tokens[0].cancelled.wait()
    XCTAssertEqual(store.observationState, .subscribing(attempt: 1))
    await second.resume.open()
    await TeraScopeFixtures.eventually { store.observationState == .active }
    let attempts = await backend.counts[.subscribe]
    await store.start()
    let after = await backend.counts[.subscribe]
    XCTAssertEqual(attempts, after)
    store.stop()
    _ = try await client.stop()
  }

  func testFailedObservationDelayClearsItsOwnerSoStartResubscribes() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let observation = TeraStoreObservation()
    var state = TeraRuntimeObservationState.inactive
    let pause = await backend.pause(.subscribe, fails: true)
    observation.start(
      client: client, buffer: (capacity: 8, delay: { _ in throw CancellationError() }),
      state: { state = $0 }, accepts: { _ in true }, refresh: { _ in }
    )
    await pause.entered.wait()
    await pause.resume.open()
    await TeraScopeFixtures.eventually { !observation.isActive }
    guard case .retrying = state else { return XCTFail("The failed retry remains visible") }
    observation.start(
      client: client, buffer: (capacity: 8, delay: { _ in throw CancellationError() }),
      state: { state = $0 }, accepts: { _ in true }, refresh: { _ in }
    )
    await TeraScopeFixtures.eventually { state == .active }
    let count = await backend.counts[.subscribe]
    XCTAssertEqual(count, 2)
    observation.stop()
    _ = try await client.stop()
  }
}
