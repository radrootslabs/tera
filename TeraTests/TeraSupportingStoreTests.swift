@testable import TeraApp
import XCTest

final class TeraSupportingStoreTests: XCTestCase {
    @MainActor
    func testSearchUsesCurrentContextDeduplicatesAndClearsEmptyQueries() async throws {
        let backend = SupportingBackend()
        let client = try await Self.startedClient(backend)
        let store = TeraSearchStore(
          runtimeClient: client,
          clock: .fixed(unixSeconds: 1_800_000_000)
        )
        store.configure(context: Self.context(id: "farm"))
        store.updateQuery(" carrots ")

        await store.search()

        XCTAssertEqual(store.state, .loaded)
        XCTAssertEqual(store.results.map(\.id), ["card", "profile"])
        let request = await backend.lastSearchRequest()
        XCTAssertEqual(request?.contextID, "farm")
        XCTAssertEqual(request?.query, "carrots")
        store.updateQuery("   ")
        XCTAssertEqual(store.state, .idle)
        XCTAssertTrue(store.results.isEmpty)
        _ = try await client.stop()
    }

    @MainActor
    func testSearchContextChangeFencesLateResults() async throws {
        let backend = SupportingBackend(searchDelayNanoseconds: 40_000_000)
        let client = try await Self.startedClient(backend)
        let store = TeraSearchStore(
          runtimeClient: client,
          clock: .fixed(unixSeconds: 1_800_000_000)
        )
        store.configure(context: Self.context(id: "first"))
        store.updateQuery("carrots")

        let search = Task { await store.search() }
        try await Task.sleep(nanoseconds: 2_000_000)
        store.configure(context: Self.context(id: "second"))
        await search.value

        XCTAssertTrue(store.results.isEmpty)
        XCTAssertEqual(store.state, .idle)
        _ = try await client.stop()
    }

    @MainActor
    func testMePreservesAdoptedProfileFieldsAndCurrentCards() async throws {
        let backend = SupportingBackend()
        let client = try await Self.startedClient(backend)
        let store = TeraMeStore(
          runtimeClient: client,
          clock: .fixed(unixSeconds: 1_800_000_000)
        )
        store.configure(context: Self.context(id: "farm"))

        await store.start()

        let snapshot = try XCTUnwrap(store.snapshot)
        XCTAssertEqual(snapshot.profile?.displayName, "Moss Farm")
        XCTAssertEqual(snapshot.profile?.about, "Local roots")
        XCTAssertEqual(snapshot.profile?.nip05, "moss@example.com")
        XCTAssertEqual(snapshot.profile?.website, "https://moss.example")
        XCTAssertEqual(snapshot.profile?.lightningAddress, "moss@example.com")
        XCTAssertEqual(snapshot.cards.map(\.id), ["card"])
        store.stop()
        _ = try await client.stop()
    }

    func testVisualIdentityIsStableAndKeyBound() {
        let first = TeraStableVisualIdentity(publicKeyHex: String(repeating: "a", count: 64))
        let repeated = TeraStableVisualIdentity(publicKeyHex: String(repeating: "a", count: 64))
        let second = TeraStableVisualIdentity(publicKeyHex: String(repeating: "b", count: 64))

        XCTAssertEqual(first, repeated)
        XCTAssertNotEqual(first.digestHex, second.digestHex)
        XCTAssertTrue((0 ..< 12).contains(first.paletteIndex))
    }

    @MainActor
    func testSettingsRoundTripUsesTypedRuntimeAndReportsReconfigurationEffects() async throws {
        let backend = SupportingBackend()
        let client = try await Self.startedClient(backend)
        let store = TeraSettingsStore(runtimeClient: client)

        await store.load(profile: nil)
        XCTAssertEqual(store.settings?.identity.identities.count, 1)
        store.networkEnvironment = .physicalDevice
        store.relays = [
          TeraRelayPreference(url: "wss://radroots.org/", access: .readWrite),
          TeraRelayPreference(url: "wss://read.example/", access: .readOnly),
        ]
        store.blossomPrimaryOrigin = "https://blossom.radroots.org"
        store.allowCellularUploads = false
        store.mediaCacheMegabytes = 512
        store.mediaCacheArtifacts = 2000

        let restartRequired = await store.saveSettings()

        XCTAssertTrue(restartRequired)
        XCTAssertEqual(store.settings?.revision, 2)
        XCTAssertEqual(store.settings?.relays.map(\.access), [.readWrite, .readOnly])
        XCTAssertEqual(store.settings?.mediaCacheBytes, 512 * 1_048_576)
        XCTAssertEqual(store.settings?.mediaCacheArtifacts, 2000)
        XCTAssertEqual(store.message, "Settings saved; required changes: runtime restart, outbox requeue, media cache refresh.")
        XCTAssertNil(store.failureCode)
        _ = try await client.stop()
    }

    @MainActor
    func testInvalidSettingsFailClosedWithoutReplacingLastAcceptedState() async throws {
        let backend = SupportingBackend()
        let client = try await Self.startedClient(backend)
        let store = TeraSettingsStore(runtimeClient: client)
        await store.load(profile: nil)
        store.relays = [TeraRelayPreference(url: "https://not-a-relay.example", access: .readWrite)]

        let restartRequired = await store.saveSettings()

        XCTAssertFalse(restartRequired)
        XCTAssertEqual(store.failureCode, "invalid_relay_endpoint")
        XCTAssertEqual(store.settings?.revision, 1)
        let revision = await backend.settingsRevision()
        XCTAssertEqual(revision, 1)
        _ = try await client.stop()
    }

    @MainActor
    func testProfileEditingUsesDurableTypedStatus() async throws {
        let backend = SupportingBackend()
        let client = try await Self.startedClient(backend)
        let store = TeraSettingsStore(runtimeClient: client)
        store.profileName = "moss"
        store.profileDisplayName = "Moss Farm"
        store.profileAbout = "Local roots"
        store.profileNip05 = "moss@example.com"

        await store.saveProfile()
        XCTAssertEqual(store.profileStatus?.state, .queued)
        await store.advanceProfile()
        XCTAssertEqual(store.profileStatus?.state, .complete)
        XCTAssertEqual(store.profileStatus?.settlement?.deliverySatisfied, 1)
        _ = try await client.stop()
    }

    static func startedClient(_ backend: SupportingBackend) async throws -> TeraRuntimeClient {
        let client = TeraRuntimeClient { _ in
            await TeraRuntimeBackendStart(backend: backend, snapshot: backend.snapshotValue())
        }
        _ = try await client.start(configuration: configuration())
        return client
    }

    private static func configuration() -> TeraRuntimeLaunchConfiguration {
        TeraRuntimeLaunchConfiguration(
          applicationSupportDirectory: "/tmp/radroots-supporting-tests",
          publicKeyHex: String(repeating: "a", count: 64),
          sourceGenerationHex: String(repeating: "c", count: 64),
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
            bundleIdentifier: "org.radroots.supporting-tests",
            version: "0.1.0-alpha",
            buildNumber: "1",
            buildSHA: nil
          ),
          signerGeneration: "supporting-tests",
          signer: SupportingSigner(),
          adoptBootstrapSettings: false
        )
    }

    private static func context(id: String) -> TeraLocalNetwork {
        TeraLocalNetwork(
          schemaVersion: 1,
          id: id,
          label: "Farm",
          relayURLs: ["ws://127.0.0.1:7447"],
          locality: "Metchosin",
          followedAuthors: [],
          generation: 1
        )
    }
}

private struct SupportingSigner: TeraRuntimeSigner {
    func availability() -> TeraRuntimeSignerAvailability {
        .ready
    }

    func sign(_: TeraRuntimeSigningRequest) -> TeraRuntimeSigningOutcome {
        .failed
    }
}
