@testable import TeraApp
import XCTest

final class TeraRuntimeClientTests: XCTestCase {
    func testConcurrentStartSharesOneBackend() async throws {
        let harness = RuntimeHarness()
        let client = TeraRuntimeClient(factory: harness.start)
        let configuration = makeConfiguration(generation: "01")

        let snapshots = try await withThrowingTaskGroup(of: TeraRuntimeSnapshot.self) { group in
            for _ in 0 ..< 64 {
                group.addTask {
                    try await client.start(configuration: configuration)
                }
            }
            return try await group.reduce(into: []) { $0.append($1) }
        }

        XCTAssertEqual(snapshots.count, 64)
        let startCount = await harness.startCount()
        let lifecycle = await client.lifecycle()
        XCTAssertEqual(startCount, 1)
        XCTAssertEqual(lifecycle, .running(generation: TeraSessionGeneration(rawValue: 1)))
        _ = try await client.stop()
    }

    func testIndependentSubscriptionsUseBoundedNewestBuffers() async throws {
        let harness = RuntimeHarness()
        let client = TeraRuntimeClient(factory: harness.start)
        _ = try await client.start(configuration: makeConfiguration(generation: "02"))
        let first = try await client.changes(bufferCapacity: 2)
        let second = try await client.changes(bufferCapacity: 4)

        for generation in 1 ... 10 {
            await harness.emitRevision(UInt64(generation))
        }

        var firstIterator = first.makeAsyncIterator()
        var secondIterator = second.makeAsyncIterator()
        let firstValues = await [firstIterator.next(), firstIterator.next()].compactMap {
            $0?.revision.rawValue
        }
        let secondValues = await [
          secondIterator.next(),
          secondIterator.next(),
          secondIterator.next(),
          secondIterator.next(),
        ].compactMap { $0?.revision.rawValue }

        XCTAssertEqual(firstValues, [9, 10])
        XCTAssertEqual(secondValues, [7, 8, 9, 10])
        _ = try await client.stop()
        let cancelCount = await harness.cancelCount()
        XCTAssertEqual(cancelCount, 2)
    }

    func testOverlappingStopsAwaitOneTypedShutdownFailure() async throws {
        let failure = TeraRuntimeFailure.local(
          operation: "test.shutdown",
          code: "test.shutdown_failed",
          safeMessage: "Shutdown did not complete."
        )
        let harness = RuntimeHarness(shutdownFailure: failure)
        let client = TeraRuntimeClient(factory: harness.start)
        _ = try await client.start(configuration: makeConfiguration(generation: "03"))

        let results = await withTaskGroup(of: Result<TeraRuntimeShutdownReceipt, Error>.self) {
            group in
            for _ in 0 ..< 32 {
                group.addTask {
                    do {
                        return try await .success(client.stop())
                    } catch {
                        return .failure(error)
                    }
                }
            }
            return await group.reduce(into: []) { $0.append($1) }
        }

        let shutdownCount = await harness.shutdownCount()
        XCTAssertEqual(shutdownCount, 1)
        XCTAssertEqual(results.count, 32)
        for result in results {
            guard case let .failure(error) = result else {
                return XCTFail("Expected every waiter to receive the shutdown failure")
            }
            XCTAssertEqual(error as? TeraRuntimeClientError, .shutdown(failure))
        }
    }

    func testNewConfigurationSupersedesAndClosesSlowStartup() async throws {
        let harness = RuntimeHarness(startDelayNanoseconds: 30_000_000)
        let client = TeraRuntimeClient(factory: harness.start)
        let firstConfiguration = makeConfiguration(generation: "04")
        let secondConfiguration = makeConfiguration(generation: "05")

        let first = Task {
            try await client.start(configuration: firstConfiguration)
        }
        try await Task.sleep(nanoseconds: 2_000_000)
        let second = Task {
            try await client.start(configuration: secondConfiguration)
        }

        do {
            _ = try await first.value
            XCTFail("The older startup should be superseded")
        } catch {
            XCTAssertEqual(error as? TeraRuntimeClientError, .superseded)
        }
        let snapshot = try await second.value
        XCTAssertEqual(snapshot.identity.publicKeyHex, secondConfiguration.publicKeyHex)
        let startCount = await harness.startCount()
        let supersededShutdownCount = await harness.shutdownCount()
        XCTAssertEqual(startCount, 2)
        XCTAssertEqual(supersededShutdownCount, 1)
        _ = try await client.stop()
        let finalShutdownCount = await harness.shutdownCount()
        XCTAssertEqual(finalShutdownCount, 2)
    }

    func testBufferCapacityAndStoppedSubscriptionFailClosed() async throws {
        let harness = RuntimeHarness()
        let client = TeraRuntimeClient(factory: harness.start)
        do {
            _ = try await client.changes()
            XCTFail("A stopped runtime cannot subscribe")
        } catch {
            XCTAssertEqual(error as? TeraRuntimeClientError, .notRunning)
        }

        _ = try await client.start(configuration: makeConfiguration(generation: "06"))
        for invalidCapacity in [0, 65] {
            do {
                _ = try await client.changes(bufferCapacity: invalidCapacity)
                XCTFail("Invalid capacities must fail closed")
            } catch {
                XCTAssertEqual(error as? TeraRuntimeClientError, .invalidBufferCapacity)
            }
        }
        _ = try await client.stop()
    }

    func testStartupDeadlineReturnsPromptlyAndClosesLateBackend() async throws {
        let harness = RuntimeHarness(startDelayNanoseconds: 60_000_000)
        let client = TeraRuntimeClient(
          factory: harness.start,
          deadlines: makeDeadlines(startup: 5_000_000)
        )

        do {
            _ = try await client.start(configuration: makeConfiguration(generation: "07"))
            XCTFail("A startup that exceeds its deadline must fail")
        } catch let TeraRuntimeClientError.startup(failure) {
            XCTAssertEqual(failure.code, "ios.runtime.deadline_exceeded")
            XCTAssertTrue(failure.operationID?.hasSuffix("-startup") == true)
        }

        try await Task.sleep(nanoseconds: 80_000_000)
        let shutdownCount = await harness.shutdownCount()
        XCTAssertEqual(shutdownCount, 1)
    }

    func testStopDrainsCancellationIgnoringStartup() async throws {
        let harness = RuntimeHarness(startDelayNanoseconds: 100_000_000)
        let client = TeraRuntimeClient(factory: harness.start)
        let configuration = makeConfiguration(generation: "08")
        let startup = Task {
            try await client.start(configuration: configuration)
        }
        try await Task.sleep(nanoseconds: 2_000_000)

        let receipt = try await client.stop()

        XCTAssertEqual(receipt, .alreadyStopped)
        let completedCloses = await harness.shutdownCount()
        XCTAssertEqual(completedCloses, 1)
        do {
            _ = try await startup.value
            XCTFail("The cancelled startup must not claim backend ownership")
        } catch {
            XCTAssertEqual(error as? TeraRuntimeClientError, .superseded)
        }
        let shutdownCount = await harness.shutdownCount()
        XCTAssertEqual(shutdownCount, 1)
    }

    func testCancellingOneStartupWaiterDoesNotCancelSharedStartup() async throws {
        let harness = RuntimeHarness(startDelayNanoseconds: 40_000_000)
        let client = TeraRuntimeClient(factory: harness.start)
        let configuration = makeConfiguration(generation: "0c")
        let cancelledWaiter = Task {
            try await client.start(configuration: configuration)
        }
        let survivingWaiter = Task {
            try await client.start(configuration: configuration)
        }
        try await Task.sleep(nanoseconds: 2_000_000)
        cancelledWaiter.cancel()

        do {
            _ = try await cancelledWaiter.value
            XCTFail("The cancelled waiter must return promptly")
        } catch let TeraRuntimeClientError.startup(failure) {
            XCTAssertEqual(failure.code, "ios.runtime.cancelled")
        }
        let snapshot = try await survivingWaiter.value
        XCTAssertEqual(snapshot.identity.publicKeyHex, configuration.publicKeyHex)
        let startCount = await harness.startCount()
        XCTAssertEqual(startCount, 1)
        _ = try await client.stop()
    }

    func testOperationDeadlineAndLateValueCannotEscape() async throws {
        let harness = RuntimeHarness(snapshotDelayNanoseconds: 60_000_000)
        let client = TeraRuntimeClient(
          factory: harness.start,
          deadlines: makeDeadlines(operation: 5_000_000)
        )
        _ = try await client.start(configuration: makeConfiguration(generation: "09"))

        do {
            _ = try await client.snapshot()
            XCTFail("A runtime operation that exceeds its deadline must fail")
        } catch let TeraRuntimeClientError.status(failure) {
            XCTAssertEqual(failure.code, "ios.runtime.deadline_exceeded")
            XCTAssertTrue(failure.operationID?.hasSuffix("-operation") == true)
        }

        _ = try await client.stop()
    }

    func testSuspendCancelsTrackedOperationWithoutClosingRuntime() async throws {
        let harness = RuntimeHarness(snapshotDelayNanoseconds: 60_000_000)
        let client = TeraRuntimeClient(factory: harness.start)
        let configuration = makeConfiguration(generation: "0d")
        _ = try await client.start(configuration: configuration)
        let pendingSnapshot = Task {
            try await client.snapshot()
        }
        try await Task.sleep(nanoseconds: 2_000_000)

        await client.suspend()

        do {
            _ = try await pendingSnapshot.value
            XCTFail("Suspension must cancel tracked presentation work")
        } catch let TeraRuntimeClientError.status(failure) {
            XCTAssertEqual(failure.code, "ios.runtime.cancelled")
        }
        let lifecycle = await client.lifecycle()
        XCTAssertEqual(lifecycle, .running(generation: TeraSessionGeneration(rawValue: 1)))
        try await Task.sleep(nanoseconds: 80_000_000)
        let recoveredSnapshot = try await client.snapshot()
        XCTAssertEqual(recoveredSnapshot.identity.publicKeyHex, configuration.publicKeyHex)
        _ = try await client.stop()
    }

    func testShutdownDeadlineQuarantinesBackendUntilLateSuccess() async throws {
        let harness = RuntimeHarness(shutdownDelayNanoseconds: 60_000_000)
        let client = TeraRuntimeClient(
          factory: harness.start,
          deadlines: makeDeadlines(shutdown: 5_000_000)
        )
        _ = try await client.start(configuration: makeConfiguration(generation: "0a"))

        do {
            _ = try await client.stop()
            XCTFail("A shutdown that exceeds its deadline must fail")
        } catch let TeraRuntimeClientError.shutdown(failure) {
            XCTAssertEqual(failure.code, "ios.runtime.deadline_exceeded")
        }
        if case .failed = await client.lifecycle() {
            // Expected while the detached backend remains quarantined.
        } else {
            XCTFail("The timed-out backend must remain quarantined")
        }

        try await Task.sleep(nanoseconds: 80_000_000)
        let finalLifecycle = await client.lifecycle()
        let finalReceipt = try await client.stop()
        XCTAssertEqual(finalLifecycle, .stopped)
        XCTAssertEqual(finalReceipt, .alreadyStopped)
    }

    @MainActor
    func testObservationFailureIsVisibleAndRecovers() async throws {
        let harness = RuntimeHarness(subscriptionFailures: 1)
        let client = TeraRuntimeClient(factory: harness.start)
        let snapshot = try await client.start(configuration: makeConfiguration(generation: "0b"))
        let store = TeraTodayStore(
          runtimeClient: client,
          contexts: [.defaultContext(snapshot: snapshot)],
          observationDelay: { _ in try await Task.sleep(nanoseconds: 20_000_000) }
        )

        await store.start()
        try await waitUntil {
            if case .retrying = store.observationState {
                return true
            }
            return false
        }
        try await waitUntil { store.observationState == .active }

        let subscriptionAttempts = await harness.subscriptionAttemptCount()
        XCTAssertGreaterThanOrEqual(subscriptionAttempts, 2)
        store.stop()
        XCTAssertEqual(store.observationState, .stopped)
        _ = try await client.stop()
    }

    private func makeDeadlines(
      startup: UInt64 = 1_000_000_000,
      operation: UInt64 = 1_000_000_000,
      subscription: UInt64 = 1_000_000_000,
      shutdown: UInt64 = 1_000_000_000
    ) -> TeraRuntimeDeadlinePolicy {
        TeraRuntimeDeadlinePolicy(
          startupNanoseconds: startup,
          operationNanoseconds: operation,
          subscriptionNanoseconds: subscription,
          shutdownNanoseconds: shutdown
        )
    }

    @MainActor
    private func waitUntil(
        _ predicate: @escaping @MainActor () -> Bool
    ) async throws {
        for _ in 0 ..< 200 {
            if predicate() {
                return
            }
            try await Task.sleep(nanoseconds: 1_000_000)
        }
        XCTAssertTrue(predicate())
    }

    func makeConfiguration(generation: String) -> TeraRuntimeLaunchConfiguration {
        TeraRuntimeLaunchConfiguration(
          applicationSupportDirectory: "/tmp/radroots-runtime-tests-\(generation)",
          publicKeyHex: String(repeating: generation, count: 32),
          sourceGenerationHex: String(repeating: generation, count: 32),
          sourceGenerationCreatedAtUnixMilliseconds: 1,
          protectedData: .available,
          networkProfile: .simulator,
          writableRelays: ["ws://127.0.0.1:7447"],
          blossom: TeraBlossomEndpointConfiguration(
            hostKind: .simulator,
            endpointAuthority: .loopbackDevelopment,
            primaryOrigin: "http://127.0.0.1:3000",
            fallbackOrigins: []
          ),
          app: TeraRuntimeAppMetadata(
            bundleIdentifier: "org.radroots.tests",
            version: "0.1.0-alpha",
            buildNumber: "1",
            buildSHA: nil
          ),
          signerGeneration: generation,
          signer: TestRuntimeSigner(),
          adoptBootstrapSettings: false
        )
    }
}
