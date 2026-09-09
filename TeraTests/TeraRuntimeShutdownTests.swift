@testable import TeraApp
import XCTest

final class TeraRuntimeShutdownTests: XCTestCase {
  func testTimeoutRetainsActualOperationAndRepeatedCloseDrainsIt() async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "70")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let pause = ResourceTestPause()
    let client = TeraRuntimeClient(
      factory: { _ in await backend.start() },
      deadlines: TeraRuntimeDeadlinePolicy(shutdownNanoseconds: 1_000_000)
    )
    _ = try await client.start(configuration: configuration)
    await backend.pauseSnapshot(pause)
    let operation = Task { try await client.snapshot() }
    await pause.entered.wait()
    await assertShutdownDeadline(client)
    let prematureCloses = await backend.shutdownCount
    XCTAssertEqual(prematureCloses, 0)
    await assertAdmissionClosed(client)
    await assertShutdownDeadline(client)
    let duplicateCloses = await backend.shutdownCount
    XCTAssertEqual(duplicateCloses, 0)
    await pause.resume.open()
    _ = await operation.result
    await awaitClosed(backend)
    _ = try await client.stop()
    let closes = await backend.shutdownCount
    XCTAssertEqual(closes, 1)
  }

  func testCancellingCloseWaiterKeepsOneSharedNativeClose() async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "71")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let pause = ResourceTestPause()
    let client = TeraRuntimeClient(factory: { _ in await backend.start() })
    _ = try await client.start(configuration: configuration)
    await backend.pauseShutdown(pause)
    let cancelled = Task { try await client.stop() }
    await pause.entered.wait()
    cancelled.cancel()
    do {
      _ = try await cancelled.value
      XCTFail("The caller should stop waiting")
    } catch let TeraRuntimeClientError.shutdown(failure) {
      XCTAssertEqual(failure.code, "ios.runtime.cancelled")
    }
    await assertAdmissionClosed(client)
    let survivor = Task { try await client.stop() }
    await pause.resume.open()
    let receipt = try await survivor.value
    XCTAssertEqual(receipt.state, "closed")
    _ = try await client.stop()
    let closes = await backend.shutdownCount
    XCTAssertEqual(closes, 1)
  }

  func testProtectedStorageFailureRetainsBackendForExplicitRetry() async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "72")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let failure = TeraRuntimeFailure.local(
      operation: "test.close", code: "protected_data_unavailable", safeMessage: "Protected data unavailable"
    )
    let client = TeraRuntimeClient(factory: { _ in await backend.start() })
    _ = try await client.start(configuration: configuration)
    await backend.failShutdownOnce(failure)
    do {
      _ = try await client.stop()
      XCTFail("Close failure must survive")
    } catch let TeraRuntimeClientError.shutdown(observed) { XCTAssertEqual(observed, failure) }
    await assertAdmissionClosed(client)
    let receipt = try await client.stop()
    XCTAssertFalse(receipt.alreadyClosed)
    let closes = await backend.shutdownCount
    XCTAssertEqual(closes, 2)
  }

  func testShutdownWaitsForLateStartupAndItsNativeCleanup() async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "73")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let creation = ResourceTestPause()
    let cleanup = ResourceTestPause()
    await backend.pauseShutdown(cleanup)
    let client = TeraRuntimeClient(
      factory: { _ in await creation.wait(); return await backend.start() },
      deadlines: TeraRuntimeDeadlinePolicy(shutdownNanoseconds: 1_000_000)
    )
    let start = Task { try await client.start(configuration: configuration) }
    await creation.entered.wait()
    await assertShutdownDeadline(client)
    _ = await start.result
    do {
      _ = try await client.start(configuration: configuration)
      XCTFail("A new startup cannot pass outstanding cleanup")
    } catch let TeraRuntimeClientError.shutdown(failure) {
      XCTAssertEqual(failure.code, "ios.runtime.deadline_exceeded")
    }
    await creation.resume.open()
    await cleanup.entered.wait()
    await assertShutdownDeadline(client)
    await cleanup.resume.open()
    await awaitClosed(backend)
    _ = try await client.stop()
    let closes = await backend.shutdownCount
    XCTAssertEqual(closes, 1)
  }

  func testFailedLateStartupCleanupIsRetainedAndRetried() async throws {
    let pause = ResourceTestPause()
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "74")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    await backend.failShutdownOnce(.local(operation: "test.close", code: "protected_data_unavailable", safeMessage: "Locked"))
    let client = TeraRuntimeClient(factory: { _ in await pause.wait(); return await backend.start() })
    let start = Task { try await client.start(configuration: configuration) }
    await pause.entered.wait()
    start.cancel()
    _ = await start.result
    await pause.resume.open()
    // The cleanup owner may already have observed the first failure. Either
    // close reports it, or this explicit request resumes that failed cleanup.
    do {
      _ = try await client.stop()
    } catch let TeraRuntimeClientError.shutdown(failure) {
      XCTAssertEqual(failure.code, "protected_data_unavailable")
      _ = try await client.stop()
    }
    let closes = await backend.shutdownCount
    XCTAssertEqual(closes, 2)
    let lifecycle = await client.lifecycle()
    XCTAssertEqual(lifecycle, .stopped)
  }

  func testLifecycleBridgeSharesCloseAndRetainsFailedRegistration() async {
    let bridge = TeraLifecycleBridge()
    let attempt = ShutdownAttemptFixture()
    await bridge.register { await attempt.close() }
    let first = Task { await bridge.requestShutdown() }
    await attempt.pause.entered.wait()
    first.cancel()
    let second = Task { await bridge.requestShutdown() }
    await attempt.pause.resume.open()
    await first.value
    await second.value
    await bridge.requestShutdown()
    await bridge.requestShutdown()
    let count = await attempt.calls
    XCTAssertEqual(count, 2)
  }

  private func assertAdmissionClosed(_ client: TeraRuntimeClient) async {
    do {
      _ = try await client.snapshot()
      XCTFail("Closing runtime admitted a command")
    } catch { XCTAssertEqual(error as? TeraRuntimeClientError, .notRunning) }
  }

  private func assertShutdownDeadline(_ client: TeraRuntimeClient) async {
    do {
      _ = try await client.stop()
      XCTFail("Outstanding work cannot report closed")
    } catch let TeraRuntimeClientError.shutdown(failure) {
      XCTAssertEqual(failure.code, "ios.runtime.deadline_exceeded")
    } catch { XCTFail("Unexpected close failure: \(error)") }
  }

  private func awaitClosed(_ backend: ResourceTestBackend) async {
    let completed = expectation(description: "Native close completed")
    let wait = Task { await backend.closed.wait(); completed.fulfill() }
    await fulfillment(of: [completed], timeout: 2)
    await backend.closed.open()
    await wait.value
  }
}

private actor ShutdownAttemptFixture {
  let pause = ResourceTestPause()
  private(set) var calls = 0
  func close() async -> Bool {
    calls += 1
    if calls == 1 {
      await pause.wait(); return false
    }
    return true
  }
}
