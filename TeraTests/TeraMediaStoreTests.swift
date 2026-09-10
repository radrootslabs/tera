@testable import TeraApp
import XCTest

final class TeraMediaStoreTests: XCTestCase {
    @MainActor
    func testVerifiedReferenceLoadsOnlyDigestBoundCachedBytes() async throws {
        let artifact = makeArtifact()
        let backend = MediaBackend(cachedArtifact: artifact)
        let client = try await startedClient(backend: backend)
        let store = TeraMediaStore(runtimeClient: client)

        store.load(media: makeReference(verification: .verified), context: makeContext())

        await waitUntil {
            store.state(for: makeReference(verification: .verified), context: makeContext())
                == .ready(artifact)
        }
        let calls = await backend.callCounts()
        XCTAssertEqual(calls.cached, 1)
        XCTAssertEqual(calls.retrieved, 0)
        _ = try await client.stop()
    }

    @MainActor
    func testUnavailableReferenceRetrievesAndPresentsVerifiedBytes() async throws {
        let artifact = makeArtifact()
        let backend = MediaBackend(retrievedArtifact: artifact)
        let client = try await startedClient(backend: backend)
        let store = TeraMediaStore(runtimeClient: client)
        let reference = makeReference(verification: .unavailable)

        store.load(media: reference, context: makeContext())

        await waitUntil {
            store.state(for: reference, context: makeContext()) == .ready(artifact)
        }
        let calls = await backend.callCounts()
        XCTAssertEqual(calls.cached, 0)
        XCTAssertEqual(calls.retrieved, 1)
        _ = try await client.stop()
    }

    @MainActor
    func testTransportFailurePresentsServiceUnavailableState() async throws {
        let backend = MediaBackend(
            retrievalFailure: TeraRuntimeFailure(
              schemaVersion: 1,
              code: "blossom_transport_failed",
              category: "network",
              retryable: true,
              recoveryActions: ["retry"],
              operationID: "test.media.retrieve",
              capabilityID: "blossom_source",
              safeMessage: "The photo service is offline."
            )
        )
        let client = try await startedClient(backend: backend)
        let store = TeraMediaStore(runtimeClient: client)
        let reference = makeReference(verification: .unavailable)

        store.load(media: reference, context: makeContext())

        await waitUntil {
            store.state(for: reference, context: makeContext()) == .networkUnavailable
        }
        _ = try await client.stop()
    }

    @MainActor
    func testUndecodableArtifactIsInvalidatedAndPresentedAsCorrupt() async throws {
        let artifact = makeArtifact(bytes: Data([0x00, 0x01, 0x02]))
        let backend = MediaBackend(cachedArtifact: artifact)
        let client = try await startedClient(backend: backend)
        let store = TeraMediaStore(runtimeClient: client)
        let reference = makeReference(verification: .verified)

        store.load(media: reference, context: makeContext())

        await waitUntil {
            store.state(for: reference, context: makeContext()) == .corrupt
        }
        let invalidatedArtifactIDs = await backend.invalidatedArtifactIDs()
        XCTAssertEqual(invalidatedArtifactIDs, [artifact.artifactID])
        _ = try await client.stop()
    }

    @MainActor
    private func startedClient(backend: MediaBackend) async throws -> TeraRuntimeClient {
        let client = TeraRuntimeClient { _ in
            await TeraRuntimeBackendStart(backend: backend, snapshot: backend.snapshot())
        }
        _ = try await client.start(configuration: makeConfiguration())
        return client
    }

    @MainActor
    private func waitUntil(
      attempts: Int = 100,
      condition: () -> Bool
    ) async {
        for _ in 0 ..< attempts {
            if condition() {
                return
            }
            await Task.yield()
            try? await Task.sleep(nanoseconds: 1_000_000)
        }
        XCTFail("Timed out waiting for media state transition")
    }

    private func makeConfiguration() -> TeraRuntimeLaunchConfiguration {
        TeraRuntimeLaunchConfiguration(
          applicationSupportDirectory: "/tmp/radroots-media-tests",
          publicKeyHex: String(repeating: "ab", count: 32),
          sourceGenerationHex: String(repeating: "cd", count: 32),
          sourceGenerationCreatedAtUnixMilliseconds: 1,
          protectedData: .available,
          networkProfile: .simulator,
          writableRelays: ["ws://127.0.0.1:21000"],
          blossom: TeraBlossomEndpointConfiguration(
            hostKind: .simulator,
            endpointAuthority: .loopbackDevelopment,
            primaryOrigin: "http://127.0.0.1:21100",
            fallbackOrigins: []
          ),
          app: TeraRuntimeAppMetadata(
            bundleIdentifier: "org.radroots.media-tests",
            version: "0.1.0-alpha",
            buildNumber: "1",
            buildSHA: nil
          ),
          signerGeneration: "media-tests",
          signer: MediaTestSigner(),
          adoptBootstrapSettings: false
        )
    }

    private func makeContext() -> TeraLocalNetwork {
        TeraLocalNetwork(
          schemaVersion: 1,
          id: "media-context",
          label: "Media context",
          relayURLs: ["ws://127.0.0.1:21000"],
          locality: nil,
          followedAuthors: [],
          generation: 1
        )
    }

    private func makeReference(
        verification: TeraMediaVerificationState
    ) -> TeraMediaReference {
        TeraMediaReference(
          referenceFingerprint: String(repeating: "f", count: 64),
          url: "https://blossom.example/\(String(repeating: "a", count: 64)).png",
          sha256: String(repeating: "a", count: 64),
          mediaType: "image/png",
          width: 1,
          height: 1,
          byteSize: nil,
          alt: "A farm photo",
          verification: verification
        )
    }

    private func makeArtifact(
        bytes: Data = Data(
            base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9ZJbIAAAAASUVORK5CYII="
        )!
    ) -> TeraVerifiedMediaArtifact {
        TeraVerifiedMediaArtifact(
          artifactID: String(repeating: "a", count: 64),
          bytes: bytes,
          byteSize: UInt64(bytes.count),
          mediaType: "image/png",
          width: 1,
          height: 1
        )!
    }
}

private struct MediaTestSigner: TeraRuntimeSigner {
    func availability() async -> TeraRuntimeSignerAvailability {
        .ready
    }

    func sign(_: TeraRuntimeSigningRequest) async -> TeraRuntimeSigningOutcome {
        .failed
    }
}

private actor MediaBackend: TeraRuntimeBackend {
    private var cachedArtifact: TeraVerifiedMediaArtifact?
    private let retrievedArtifact: TeraVerifiedMediaArtifact?
    private let retrievalFailure: TeraRuntimeFailure?
    private var cachedCalls = 0
    private var retrieveCalls = 0
    private var invalidatedIDs: [String] = []
    private var closed = false

    init(
      cachedArtifact: TeraVerifiedMediaArtifact? = nil,
      retrievedArtifact: TeraVerifiedMediaArtifact? = nil,
      retrievalFailure: TeraRuntimeFailure? = nil
    ) {
        self.cachedArtifact = cachedArtifact
        self.retrievedArtifact = retrievedArtifact
        self.retrievalFailure = retrievalFailure
    }

    func snapshot() -> TeraRuntimeSnapshot {
        TeraRuntimeSnapshot(
          identity: TeraRuntimeIdentity(
            publicKeyHex: String(repeating: "ab", count: 32),
            hostSignerConfigured: true
          ),
          relay: nil,
          blossomConfiguration: nil,
          blossomEvidence: nil,
          crateName: "tera_ffi",
          crateVersion: "0.1.0-alpha",
          isClosed: closed
        )
    }

    func todayPage(request _: TeraTodayPageRequest) throws -> TeraTodayPage {
        throw testFailure(operation: "test.media.today")
    }

    func refreshToday(
      context _: TeraLocalNetwork,
      nowUnixSeconds _: UInt64,
      update _: TeraTodayProjectionUpdate
    ) throws -> TeraTodaySyncReceipt {
        throw testFailure(operation: "test.media.refresh")
    }

    func retrieveMedia(
      context _: TeraLocalNetwork,
      reference _: TeraMediaReference
    ) throws -> TeraVerifiedMediaArtifact {
        retrieveCalls += 1
        if let retrievalFailure {
            throw retrievalFailure
        }
        guard let retrievedArtifact else {
            throw testFailure(operation: "test.media.retrieve")
        }
        return retrievedArtifact
    }

    func verifiedMediaArtifact(
      context _: TeraLocalNetwork,
      artifactID _: String
    ) -> TeraVerifiedMediaArtifact? {
        cachedCalls += 1
        return cachedArtifact
    }

    func invalidateMediaArtifact(
      context _: TeraLocalNetwork,
      artifactID: String
    ) -> Bool {
        invalidatedIDs.append(artifactID)
        cachedArtifact = nil
        return true
    }

    func subscribe(
      bufferCapacity _: Int,
      receive _: @escaping @Sendable (TeraRuntimeChange) async -> Void
    ) -> any TeraRuntimeSubscriptionToken {
        MediaSubscriptionToken()
    }

    func shutdown() -> TeraRuntimeShutdownReceipt {
        let alreadyClosed = closed
        closed = true
        return TeraRuntimeShutdownReceipt(state: "closed", alreadyClosed: alreadyClosed)
    }

    func callCounts() -> (cached: Int, retrieved: Int) {
        (cachedCalls, retrieveCalls)
    }

    func invalidatedArtifactIDs() -> [String] {
        invalidatedIDs
    }

    private func testFailure(operation: String) -> TeraRuntimeFailure {
        .local(
          operation: operation,
          code: "test.media.unsupported",
          safeMessage: "This test operation is unsupported."
        )
    }
}

private actor MediaSubscriptionToken: TeraRuntimeSubscriptionToken {
    func cancel() {}
}
