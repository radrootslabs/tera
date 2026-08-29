@testable import RadrootsApp
import XCTest

final class RadrootsRuntimeClientTests: XCTestCase {
    func testConcurrentStartSharesOneBackend() async throws {
        let harness = RuntimeHarness()
        let client = RadrootsRuntimeClient(factory: harness.start)
        let configuration = makeConfiguration(generation: "01")

        let snapshots = try await withThrowingTaskGroup(of: RadrootsRuntimeSnapshot.self) { group in
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
        XCTAssertEqual(lifecycle, .running(generation: 1))
        _ = try await client.stop()
    }

    func testIndependentSubscriptionsUseBoundedNewestBuffers() async throws {
        let harness = RuntimeHarness()
        let client = RadrootsRuntimeClient(factory: harness.start)
        _ = try await client.start(configuration: makeConfiguration(generation: "02"))
        let first = try await client.changes(bufferCapacity: 2)
        let second = try await client.changes(bufferCapacity: 4)

        for generation in 1 ... 10 {
            await harness.emit(change(generation: UInt64(generation)))
        }

        var firstIterator = first.makeAsyncIterator()
        var secondIterator = second.makeAsyncIterator()
        let firstValues = await [firstIterator.next(), firstIterator.next()].compactMap {
            $0?.generation
        }
        let secondValues = await [
          secondIterator.next(),
          secondIterator.next(),
          secondIterator.next(),
          secondIterator.next(),
        ].compactMap { $0?.generation }

        XCTAssertEqual(firstValues, [9, 10])
        XCTAssertEqual(secondValues, [7, 8, 9, 10])
        _ = try await client.stop()
        let cancelCount = await harness.cancelCount()
        XCTAssertEqual(cancelCount, 2)
    }

    func testOverlappingStopsAwaitOneTypedShutdownFailure() async throws {
        let failure = RadrootsRuntimeFailure.local(
          operation: "test.shutdown",
          code: "test.shutdown_failed",
          safeMessage: "Shutdown did not complete."
        )
        let harness = RuntimeHarness(shutdownFailure: failure)
        let client = RadrootsRuntimeClient(factory: harness.start)
        _ = try await client.start(configuration: makeConfiguration(generation: "03"))

        let results = await withTaskGroup(of: Result<RadrootsRuntimeShutdownReceipt, Error>.self) {
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
            XCTAssertEqual(error as? RadrootsRuntimeClientError, .shutdown(failure))
        }
    }

    func testNewConfigurationSupersedesAndClosesSlowStartup() async throws {
        let harness = RuntimeHarness(startDelayNanoseconds: 30_000_000)
        let client = RadrootsRuntimeClient(factory: harness.start)
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
            XCTAssertEqual(error as? RadrootsRuntimeClientError, .superseded)
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
        let client = RadrootsRuntimeClient(factory: harness.start)
        do {
            _ = try await client.changes()
            XCTFail("A stopped runtime cannot subscribe")
        } catch {
            XCTAssertEqual(error as? RadrootsRuntimeClientError, .notRunning)
        }

        _ = try await client.start(configuration: makeConfiguration(generation: "06"))
        for invalidCapacity in [0, 65] {
            do {
                _ = try await client.changes(bufferCapacity: invalidCapacity)
                XCTFail("Invalid capacities must fail closed")
            } catch {
                XCTAssertEqual(error as? RadrootsRuntimeClientError, .invalidBufferCapacity)
            }
        }
        _ = try await client.stop()
    }

    func testStartupDeadlineReturnsPromptlyAndClosesLateBackend() async throws {
        let harness = RuntimeHarness(startDelayNanoseconds: 60_000_000)
        let client = RadrootsRuntimeClient(
          factory: harness.start,
          deadlines: makeDeadlines(startup: 5_000_000)
        )

        do {
            _ = try await client.start(configuration: makeConfiguration(generation: "07"))
            XCTFail("A startup that exceeds its deadline must fail")
        } catch let RadrootsRuntimeClientError.startup(failure) {
            XCTAssertEqual(failure.code, "ios.runtime.deadline_exceeded")
            XCTAssertTrue(failure.operationID?.hasSuffix("-startup") == true)
        }

        try await Task.sleep(nanoseconds: 80_000_000)
        let shutdownCount = await harness.shutdownCount()
        XCTAssertEqual(shutdownCount, 1)
    }

    func testStopDoesNotWaitForCancellationIgnoringStartup() async throws {
        let harness = RuntimeHarness(startDelayNanoseconds: 100_000_000)
        let client = RadrootsRuntimeClient(factory: harness.start)
        let configuration = makeConfiguration(generation: "08")
        let startup = Task {
            try await client.start(configuration: configuration)
        }
        try await Task.sleep(nanoseconds: 2_000_000)

        let clock = ContinuousClock()
        let startedAt = clock.now
        let receipt = try await client.stop()

        XCTAssertEqual(receipt, .alreadyStopped)
        XCTAssertLessThan(startedAt.duration(to: clock.now), .milliseconds(50))
        do {
            _ = try await startup.value
            XCTFail("The cancelled startup must not claim backend ownership")
        } catch {
            XCTAssertEqual(error as? RadrootsRuntimeClientError, .superseded)
        }
        try await Task.sleep(nanoseconds: 120_000_000)
        let shutdownCount = await harness.shutdownCount()
        XCTAssertEqual(shutdownCount, 1)
    }

    func testCancellingOneStartupWaiterDoesNotCancelSharedStartup() async throws {
        let harness = RuntimeHarness(startDelayNanoseconds: 40_000_000)
        let client = RadrootsRuntimeClient(factory: harness.start)
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
        } catch let RadrootsRuntimeClientError.startup(failure) {
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
        let client = RadrootsRuntimeClient(
          factory: harness.start,
          deadlines: makeDeadlines(operation: 5_000_000)
        )
        _ = try await client.start(configuration: makeConfiguration(generation: "09"))

        do {
            _ = try await client.snapshot()
            XCTFail("A runtime operation that exceeds its deadline must fail")
        } catch let RadrootsRuntimeClientError.status(failure) {
            XCTAssertEqual(failure.code, "ios.runtime.deadline_exceeded")
            XCTAssertTrue(failure.operationID?.hasSuffix("-operation") == true)
        }

        _ = try await client.stop()
    }

    func testSuspendCancelsTrackedOperationWithoutClosingRuntime() async throws {
        let harness = RuntimeHarness(snapshotDelayNanoseconds: 60_000_000)
        let client = RadrootsRuntimeClient(factory: harness.start)
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
        } catch let RadrootsRuntimeClientError.status(failure) {
            XCTAssertEqual(failure.code, "ios.runtime.cancelled")
        }
        let lifecycle = await client.lifecycle()
        XCTAssertEqual(lifecycle, .running(generation: 1))
        try await Task.sleep(nanoseconds: 80_000_000)
        let recoveredSnapshot = try await client.snapshot()
        XCTAssertEqual(recoveredSnapshot.identity.publicKeyHex, configuration.publicKeyHex)
        _ = try await client.stop()
    }

    func testShutdownDeadlineQuarantinesBackendUntilLateSuccess() async throws {
        let harness = RuntimeHarness(shutdownDelayNanoseconds: 60_000_000)
        let client = RadrootsRuntimeClient(
          factory: harness.start,
          deadlines: makeDeadlines(shutdown: 5_000_000)
        )
        _ = try await client.start(configuration: makeConfiguration(generation: "0a"))

        do {
            _ = try await client.stop()
            XCTFail("A shutdown that exceeds its deadline must fail")
        } catch let RadrootsRuntimeClientError.shutdown(failure) {
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
        let client = RadrootsRuntimeClient(factory: harness.start)
        let snapshot = try await client.start(configuration: makeConfiguration(generation: "0b"))
        let store = RadrootsTodayStore(
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
    ) -> RadrootsRuntimeDeadlinePolicy {
        RadrootsRuntimeDeadlinePolicy(
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

    private func makeConfiguration(generation: String) -> RadrootsRuntimeLaunchConfiguration {
        RadrootsRuntimeLaunchConfiguration(
          applicationSupportDirectory: "/tmp/radroots-runtime-tests-\(generation)",
          publicKeyHex: String(repeating: generation, count: 32),
          sourceGenerationHex: String(repeating: generation, count: 32),
          sourceGenerationCreatedAtUnixMilliseconds: 1,
          protectedData: .available,
          networkProfile: .simulator,
          writableRelays: ["ws://127.0.0.1:7447"],
          blossom: RadrootsBlossomEndpointConfiguration(
            hostKind: .simulator,
            endpointAuthority: .loopbackDevelopment,
            primaryOrigin: "http://127.0.0.1:3000",
            fallbackOrigins: []
          ),
          app: RadrootsRuntimeAppMetadata(
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

    private func change(generation: UInt64) -> RadrootsRuntimeChange {
        RadrootsRuntimeChange(
          schemaVersion: 1,
          generation: generation,
          kind: .today,
          entityID: "card-\(generation)"
        )
    }
}

private struct TestRuntimeSigner: RadrootsRuntimeSigner {
    func availability() async -> RadrootsRuntimeSignerAvailability {
        .ready
    }

    func sign(_: RadrootsRuntimeSigningRequest) async -> RadrootsRuntimeSigningOutcome {
        .failed
    }
}

private actor RuntimeHarness {
    private let startDelayNanoseconds: UInt64
    private let snapshotDelayNanoseconds: UInt64
    private let shutdownDelayNanoseconds: UInt64
    private let shutdownFailure: RadrootsRuntimeFailure?
    private var subscriptionFailures: Int
    private var starts = 0
    private var shutdowns = 0
    private var cancels = 0
    private var subscriptionAttempts = 0
    private var receivers: [UUID: @Sendable (RadrootsRuntimeChange) async -> Void] = [:]

    init(
      startDelayNanoseconds: UInt64 = 0,
      snapshotDelayNanoseconds: UInt64 = 0,
      shutdownDelayNanoseconds: UInt64 = 10_000_000,
      subscriptionFailures: Int = 0,
      shutdownFailure: RadrootsRuntimeFailure? = nil
    ) {
        self.startDelayNanoseconds = startDelayNanoseconds
        self.snapshotDelayNanoseconds = snapshotDelayNanoseconds
        self.shutdownDelayNanoseconds = shutdownDelayNanoseconds
        self.subscriptionFailures = subscriptionFailures
        self.shutdownFailure = shutdownFailure
    }

    func start(
        configuration: RadrootsRuntimeLaunchConfiguration
    ) async throws -> RadrootsRuntimeBackendStart {
        starts += 1
        if startDelayNanoseconds > 0 {
            await Task.detached { [startDelayNanoseconds] in
                try? await Task.sleep(nanoseconds: startDelayNanoseconds)
            }.value
        }
        let backend = TestRuntimeBackend(
          harness: self,
          publicKeyHex: configuration.publicKeyHex,
          snapshotDelayNanoseconds: snapshotDelayNanoseconds,
          shutdownDelayNanoseconds: shutdownDelayNanoseconds,
          shutdownFailure: shutdownFailure
        )
        return await RadrootsRuntimeBackendStart(
          backend: backend,
          snapshot: backend.snapshotValue()
        )
    }

    func addReceiver(
        _ receive: @escaping @Sendable (RadrootsRuntimeChange) async -> Void
    ) throws -> UUID {
        subscriptionAttempts += 1
        if subscriptionFailures > 0 {
            subscriptionFailures -= 1
            throw RadrootsRuntimeFailure.local(
              operation: "test.subscribe",
              code: "test.subscribe_retryable",
              safeMessage: "The test subscription is temporarily unavailable."
            )
        }
        let id = UUID()
        receivers[id] = receive
        return id
    }

    func removeReceiver(id: UUID) {
        if receivers.removeValue(forKey: id) != nil {
            cancels += 1
        }
    }

    func emit(_ change: RadrootsRuntimeChange) async {
        for receive in receivers.values {
            await receive(change)
        }
    }

    func recordShutdown() {
        shutdowns += 1
    }

    func startCount() -> Int {
        starts
    }

    func shutdownCount() -> Int {
        shutdowns
    }

    func cancelCount() -> Int {
        cancels
    }

    func subscriptionAttemptCount() -> Int {
        subscriptionAttempts
    }
}

private actor TestRuntimeBackend: RadrootsRuntimeBackend {
    private let harness: RuntimeHarness
    private let snapshotValueStored: RadrootsRuntimeSnapshot
    private let snapshotDelayNanoseconds: UInt64
    private let shutdownDelayNanoseconds: UInt64
    private let shutdownFailure: RadrootsRuntimeFailure?
    private var closed = false

    init(
      harness: RuntimeHarness,
      publicKeyHex: String,
      snapshotDelayNanoseconds: UInt64,
      shutdownDelayNanoseconds: UInt64,
      shutdownFailure: RadrootsRuntimeFailure?
    ) {
        self.harness = harness
        self.snapshotDelayNanoseconds = snapshotDelayNanoseconds
        self.shutdownDelayNanoseconds = shutdownDelayNanoseconds
        self.shutdownFailure = shutdownFailure
        snapshotValueStored = RadrootsRuntimeSnapshot(
          identity: RadrootsRuntimeIdentity(
            publicKeyHex: publicKeyHex,
            hostSignerConfigured: true
          ),
          relay: nil,
          blossomConfiguration: nil,
          blossomEvidence: nil,
          crateName: "radroots_mobile_ffi",
          crateVersion: "0.1.0-alpha",
          isClosed: false
        )
    }

    func snapshotValue() -> RadrootsRuntimeSnapshot {
        snapshotValueStored
    }

    func snapshot() async throws -> RadrootsRuntimeSnapshot {
        if snapshotDelayNanoseconds > 0 {
            await Task.detached { [snapshotDelayNanoseconds] in
                try? await Task.sleep(nanoseconds: snapshotDelayNanoseconds)
            }.value
        }
        if closed {
            throw RadrootsRuntimeFailure.local(
              operation: "test.snapshot",
              code: "test.closed",
              safeMessage: "The test runtime is closed."
            )
        }
        return snapshotValueStored
    }

    func todayPage(request _: RadrootsTodayPageRequest) throws -> RadrootsTodayPage {
        throw RadrootsRuntimeFailure.local(
          operation: "test.today.page",
          code: "test.unsupported",
          safeMessage: "Today is not configured for this lifecycle test."
        )
    }

    func refreshToday(
      context _: RadrootsLocalNetwork,
      nowUnixSeconds _: UInt64,
      update _: RadrootsTodayProjectionUpdate
    ) throws -> RadrootsTodayRefreshReceipt {
        throw RadrootsRuntimeFailure.local(
          operation: "test.today.refresh",
          code: "test.unsupported",
          safeMessage: "Today is not configured for this lifecycle test."
        )
    }

    func subscribe(
      bufferCapacity _: Int,
      receive: @escaping @Sendable (RadrootsRuntimeChange) async -> Void
    ) async throws -> any RadrootsRuntimeSubscriptionToken {
        let id = try await harness.addReceiver(receive)
        return TestSubscriptionToken(harness: harness, id: id)
    }

    func shutdown() async throws -> RadrootsRuntimeShutdownReceipt {
        await harness.recordShutdown()
        await Task.detached { [shutdownDelayNanoseconds] in
            try? await Task.sleep(nanoseconds: shutdownDelayNanoseconds)
        }.value
        if let shutdownFailure {
            throw shutdownFailure
        }
        let alreadyClosed = closed
        closed = true
        return RadrootsRuntimeShutdownReceipt(state: "closed", alreadyClosed: alreadyClosed)
    }
}

private actor TestSubscriptionToken: RadrootsRuntimeSubscriptionToken {
    private let harness: RuntimeHarness
    private let id: UUID
    private var cancelled = false

    init(harness: RuntimeHarness, id: UUID) {
        self.harness = harness
        self.id = id
    }

    func cancel() async {
        guard !cancelled else { return }
        cancelled = true
        await harness.removeReceiver(id: id)
    }
}
