import Foundation
@testable import TeraApp

struct TestRuntimeSigner: TeraRuntimeSigner {
    func availability() async -> TeraRuntimeSignerAvailability {
        .ready
    }

    func sign(_: TeraRuntimeSigningRequest) async -> TeraRuntimeSigningOutcome {
        .failed
    }
}

actor RuntimeHarness {
    private var configurations: [TeraRuntimeLaunchConfiguration] = []

    func launchConfigurations() -> [TeraRuntimeLaunchConfiguration] {
      configurations
    }

    private let startDelayNanoseconds: UInt64
    private let snapshotDelayNanoseconds: UInt64
    private let shutdownDelayNanoseconds: UInt64
    private let shutdownFailure: TeraRuntimeFailure?
    private var subscriptionFailures: Int
    private var starts = 0
    private var shutdowns = 0
    private var cancels = 0
    private var subscriptionAttempts = 0
    private var receivers: [UUID: @Sendable (TeraRuntimeChange) async -> Void] = [:]

    init(
      startDelayNanoseconds: UInt64 = 0,
      snapshotDelayNanoseconds: UInt64 = 0,
      shutdownDelayNanoseconds: UInt64 = 10_000_000,
      subscriptionFailures: Int = 0,
      shutdownFailure: TeraRuntimeFailure? = nil
    ) {
        self.startDelayNanoseconds = startDelayNanoseconds
        self.snapshotDelayNanoseconds = snapshotDelayNanoseconds
        self.shutdownDelayNanoseconds = shutdownDelayNanoseconds
        self.subscriptionFailures = subscriptionFailures
        self.shutdownFailure = shutdownFailure
    }

    func start(
        configuration: TeraRuntimeLaunchConfiguration
    ) async throws -> TeraRuntimeBackendStart {
        configurations.append(configuration)
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
        return await TeraRuntimeBackendStart(
          backend: backend,
          snapshot: backend.snapshotValue()
        )
    }

    func addReceiver(
        _ receive: @escaping @Sendable (TeraRuntimeChange) async -> Void
    ) throws -> UUID {
        subscriptionAttempts += 1
        if subscriptionFailures > 0 {
            subscriptionFailures -= 1
            throw TeraRuntimeFailure.local(
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

    func emit(_ change: TeraRuntimeChange) async {
        for receive in receivers.values {
            await receive(change)
        }
    }

    func emitRevision(_ revision: UInt64) async {
        guard let configuration = configurations.last else {
            preconditionFailure("Start the fixture runtime before emitting changes")
        }
        await emit(TeraRuntimeChange(
          schemaVersion: 3,
          scope: TeraRuntimeChangeScope(publicKey: configuration.publicKeyHex,
                                        sourceGeneration: configuration.sourceGenerationHex, context: nil),
          epoch: String(repeating: "1", count: 32),
          revision: TeraProjectionRevision(rawValue: revision), delivery: .change, kind: .today, entityID: "card-\(revision)"
        ))
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

private actor TestRuntimeBackend: TeraRuntimeBackend {
    private let harness: RuntimeHarness
    private let snapshotValueStored: TeraRuntimeSnapshot
    private let snapshotDelayNanoseconds: UInt64
    private let shutdownDelayNanoseconds: UInt64
    private let shutdownFailure: TeraRuntimeFailure?
    private var closed = false

    init(
      harness: RuntimeHarness,
      publicKeyHex: String,
      snapshotDelayNanoseconds: UInt64,
      shutdownDelayNanoseconds: UInt64,
      shutdownFailure: TeraRuntimeFailure?
    ) {
        self.harness = harness
        self.snapshotDelayNanoseconds = snapshotDelayNanoseconds
        self.shutdownDelayNanoseconds = shutdownDelayNanoseconds
        self.shutdownFailure = shutdownFailure
        snapshotValueStored = TeraRuntimeSnapshot(
          identity: TeraRuntimeIdentity(
            publicKeyHex: publicKeyHex,
            hostSignerConfigured: true
          ),
          relay: nil,
          blossomConfiguration: nil,
          blossomEvidence: nil,
          crateName: "tera_ffi",
          crateVersion: "0.1.0-alpha",
          isClosed: false
        )
    }

    func snapshotValue() -> TeraRuntimeSnapshot {
        snapshotValueStored
    }

    func snapshot() async throws -> TeraRuntimeSnapshot {
        if snapshotDelayNanoseconds > 0 {
            await Task.detached { [snapshotDelayNanoseconds] in
                try? await Task.sleep(nanoseconds: snapshotDelayNanoseconds)
            }.value
        }
        if closed {
            throw TeraRuntimeFailure.local(
              operation: "test.snapshot",
              code: "test.closed",
              safeMessage: "The test runtime is closed."
            )
        }
        return snapshotValueStored
    }

    func todayPage(request _: TeraTodayPageRequest) throws -> TeraTodayPage {
        throw TeraRuntimeFailure.local(
          operation: "test.today.page",
          code: "test.unsupported",
          safeMessage: "Today is not configured for this lifecycle test."
        )
    }

    func refreshToday(
      context _: TeraLocalNetwork,
      nowUnixSeconds _: UInt64,
      update _: TeraTodayProjectionUpdate
    ) throws -> TeraTodayRefreshReceipt {
        throw TeraRuntimeFailure.local(
          operation: "test.today.refresh",
          code: "test.unsupported",
          safeMessage: "Today is not configured for this lifecycle test."
        )
    }

    func subscribe(
      bufferCapacity _: Int,
      receive: @escaping @Sendable (TeraRuntimeChange) async -> Void
    ) async throws -> any TeraRuntimeSubscriptionToken {
        let id = try await harness.addReceiver(receive)
        return TestSubscriptionToken(harness: harness, id: id)
    }

    func shutdown() async throws -> TeraRuntimeShutdownReceipt {
        await harness.recordShutdown()
        await Task.detached { [shutdownDelayNanoseconds] in
            try? await Task.sleep(nanoseconds: shutdownDelayNanoseconds)
        }.value
        if let shutdownFailure {
            throw shutdownFailure
        }
        let alreadyClosed = closed
        closed = true
        return TeraRuntimeShutdownReceipt(state: "closed", alreadyClosed: alreadyClosed)
    }
}

private actor TestSubscriptionToken: TeraRuntimeSubscriptionToken {
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
