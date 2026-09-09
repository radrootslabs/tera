@testable import TeraApp
import XCTest

final class TeraRuntimeResourceLifecycleTests: XCTestCase {
  func testStartupTimeoutClosesLateBackendWithoutResurrection() async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "30")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let pause = ResourceTestPause()
    let deadlines = TeraRuntimeDeadlinePolicy(startupNanoseconds: 1_000_000)
    let client = TeraRuntimeClient(
      factory: { _ in await pause.wait(); return await backend.start() }, deadlines: deadlines
    )
    let start = Task { try await client.start(configuration: configuration) }
    await pause.entered.wait()
    do {
      _ = try await start.value
      XCTFail("Startup deadline must end the wait")
    } catch let TeraRuntimeClientError.startup(failure) {
      XCTAssertEqual(failure.code, "ios.runtime.deadline_exceeded")
    }
    await pause.resume.open()
    await assertClosed(backend)
    guard case .failed = await client.lifecycle() else { return XCTFail("Late success cannot revive the runtime") }
    _ = try await client.stop()
  }

  func testLastCancelledStartupWaiterClosesItsLateBackend() async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "31")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let pause = ResourceTestPause()
    let client = TeraRuntimeClient(factory: { _ in await pause.wait(); return await backend.start() })
    let start = Task { try await client.start(configuration: configuration) }
    await pause.entered.wait()
    start.cancel()
    do {
      _ = try await start.value
      XCTFail("A cancelled sole waiter must not adopt the runtime")
    } catch let TeraRuntimeClientError.startup(failure) {
      XCTAssertEqual(failure.code, "ios.runtime.cancelled")
    }
    let lifecycle = await client.lifecycle()
    XCTAssertEqual(lifecycle, .stopped)
    await pause.resume.open()
    await assertClosed(backend)
    _ = try await client.stop()
    let count = await backend.shutdownCount
    XCTAssertEqual(count, 1)
  }

  func testLateSupersededStartupCannotCloseNewRuntime() async throws {
    let firstConfig = TeraRuntimeClientTests().makeConfiguration(generation: "32")
    let nextConfig = TeraRuntimeClientTests().makeConfiguration(generation: "33")
    let old = ResourceTestBackend(publicKeyHex: firstConfig.publicKeyHex)
    let current = ResourceTestBackend(publicKeyHex: nextConfig.publicKeyHex)
    let pause = ResourceTestPause()
    let client = TeraRuntimeClient(factory: { configuration in
      if configuration == firstConfig {
        await pause.wait(); return await old.start()
      }
      return await current.start()
    })
    let first = Task { try await client.start(configuration: firstConfig) }
    await pause.entered.wait()
    _ = try await client.start(configuration: nextConfig)
    do {
      _ = try await first.value
      XCTFail("Old startup must be superseded")
    } catch {
      XCTAssertEqual(error as? TeraRuntimeClientError, .superseded)
    }
    await pause.resume.open()
    await assertClosed(old)
    let snapshot = try await client.snapshot()
    let activeCloses = await current.shutdownCount
    XCTAssertEqual(snapshot.identity.publicKeyHex, nextConfig.publicKeyHex)
    XCTAssertEqual(activeCloses, 0)
    _ = try await client.stop()
  }

  func testLateSubscriptionAfterCancellationDetachesOnce() async throws {
    try await assertLateSubscription(termination: .cancel)
  }

  func testLateSubscriptionAfterTimeoutDetachesOnce() async throws {
    try await assertLateSubscription(termination: .timeout)
  }

  func testLateSubscriptionAfterStopDetachesOnce() async throws {
    try await assertLateSubscription(termination: .stop)
  }

  func testAdoptedSubscriptionFinishesAndDetachesOnceOnRepeatedStop() async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "34")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let client = TeraRuntimeClient(factory: { _ in await backend.start() })
    _ = try await client.start(configuration: configuration)
    let stream = try await client.changes()
    await backend.emit(1)
    var iterator = stream.makeAsyncIterator()
    let first = await iterator.next()
    XCTAssertEqual(first?.generation.rawValue, 1)
    _ = try await client.stop()
    _ = try await client.stop()
    await backend.emit(2)
    let final = await iterator.next()
    let cancels = await backend.token.cancelCount
    let closes = await backend.shutdownCount
    XCTAssertNil(final)
    XCTAssertEqual(cancels, 1)
    XCTAssertEqual(closes, 1)
  }

  private enum Termination { case cancel, timeout, stop }

  private func assertLateSubscription(termination: Termination) async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "35")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let pause = ResourceTestPause()
    await backend.pauseSubscription(pause)
    let deadlines = TeraRuntimeDeadlinePolicy(
      subscriptionNanoseconds: termination == .timeout ? 1_000_000 : 10_000_000_000
    )
    let client = TeraRuntimeClient(factory: { _ in await backend.start() }, deadlines: deadlines)
    _ = try await client.start(configuration: configuration)
    let creation = Task { try await client.changes() }
    await pause.entered.wait()
    switch termination {
    case .cancel: creation.cancel()
    case .stop: _ = try await client.stop()
    case .timeout: break
    }
    do {
      _ = try await creation.value
      XCTFail("Abandoned creation must not return an observer")
    } catch {
      if termination == .stop {
        XCTAssertEqual(error as? TeraRuntimeClientError, .superseded)
      } else {
        guard case let TeraRuntimeClientError.subscription(failure) = error else { throw error }
        XCTAssertEqual(failure.code, termination == .timeout ? "ios.runtime.deadline_exceeded" : "ios.runtime.cancelled")
      }
    }
    await pause.resume.open()
    let detached = expectation(description: "Late token detached")
    let wait = Task { await backend.token.cancelled.wait(); detached.fulfill() }
    await fulfillment(of: [detached], timeout: 2)
    await backend.token.cancelled.open()
    await wait.value
    _ = try await client.stop()
    let count = await backend.token.cancelCount
    XCTAssertEqual(count, 1)
  }

  private func assertClosed(_ backend: ResourceTestBackend) async {
    let closed = expectation(description: "Late backend closed")
    let wait = Task { await backend.closed.wait(); closed.fulfill() }
    await fulfillment(of: [closed], timeout: 2)
    await backend.closed.open()
    await wait.value
    let count = await backend.shutdownCount
    XCTAssertEqual(count, 1)
  }
}
