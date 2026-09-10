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

    private static func startedClient(_ backend: SupportingBackend) async throws -> TeraRuntimeClient {
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

private actor SupportingBackend: TeraRuntimeBackend {
    struct SearchRequest: Sendable {
        let contextID: String
        let query: String
    }

    private let searchDelayNanoseconds: UInt64
    private var request: SearchRequest?
    private var closed = false
    private var settings = SupportingBackend.initialSettings()
    private var savedProfile: TeraProfileMetadataInput?

    init(searchDelayNanoseconds: UInt64 = 0) {
        self.searchDelayNanoseconds = searchDelayNanoseconds
    }

    func snapshotValue() -> TeraRuntimeSnapshot {
        TeraRuntimeSnapshot(
          identity: TeraRuntimeIdentity(
            publicKeyHex: String(repeating: "a", count: 64),
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

    func snapshot() -> TeraRuntimeSnapshot {
        snapshotValue()
    }

    func todayPage(request _: TeraTodayPageRequest) throws -> TeraTodayPage {
        throw unsupported()
    }

    func refreshToday(
      context _: TeraLocalNetwork,
      nowUnixSeconds _: UInt64,
      update _: TeraTodayProjectionUpdate, backfillCursor _: String?
    ) throws -> TeraTodaySyncReceipt {
        throw unsupported()
    }

    func search(
      context: TeraLocalNetwork,
      query: String,
      limit _: UInt16,
      asOfUnixSeconds _: UInt64
    ) async throws -> [TeraSearchResult] {
        request = SearchRequest(contextID: context.id, query: query)
        if searchDelayNanoseconds > 0 {
            try await Task.sleep(nanoseconds: searchDelayNanoseconds)
        }
        let card = Self.card()
        let profile = Self.profile()
        return [
          TeraSearchResult(type: .card, id: "card", card: card, profile: nil),
          TeraSearchResult(type: .card, id: "card", card: card, profile: nil),
          TeraSearchResult(type: .profile, id: "profile", card: nil, profile: profile),
        ]
    }

    func me(
      context _: TeraLocalNetwork,
      asOfUnixSeconds _: UInt64
    ) -> TeraMeSnapshot {
        TeraMeSnapshot(
          publicKey: String(repeating: "a", count: 64),
          profile: Self.profile(),
          cards: [Self.card()]
        )
    }

    func mobileSettings() -> TeraMobileSettings {
        settings
    }

    func replaceMobileSettings(
        input: TeraReplaceSettings
    ) throws -> TeraSettingsTransition {
        guard input.expectedRevision == settings.revision else {
            throw failure(code: "settings_revision_conflict")
        }
        guard input.relays.allSatisfy({ $0.url.hasPrefix("ws://") || $0.url.hasPrefix("wss://") }) else {
            throw failure(code: "invalid_relay_endpoint")
        }
        settings = TeraMobileSettings(
          revision: settings.revision + 1,
          identity: settings.identity,
          networkEnvironment: input.networkEnvironment,
          relays: input.relays,
          blossomAuthority: input.blossomAuthority,
          blossomPrimaryOrigin: input.blossomPrimaryOrigin,
          blossomFallbackOrigins: input.blossomFallbackOrigins,
          allowCellularDownloads: input.allowCellularDownloads,
          allowCellularUploads: input.allowCellularUploads,
          allowBackgroundTransfers: input.allowBackgroundTransfers,
          mediaCacheBytes: input.mediaCacheBytes,
          mediaCacheArtifacts: input.mediaCacheArtifacts
        )
        return TeraSettingsTransition(
          settings: settings,
          runtimeRestartRequired: true,
          outboxRequeueRequired: true,
          mediaCacheInvalidationRequired: true
        )
    }

    func saveProfileMetadata(input: TeraProfileMetadataInput) -> TeraProfileStatus {
        savedProfile = input
        return profileStatus(state: .queued, revision: 1, settlement: nil)
    }

    func advanceProfile(operationID _: String) -> TeraProfileStatus {
        profileStatus(
          state: .complete,
          revision: 2,
          settlement: TeraOperationSettlement(
            artifacts: 0,
            signed: 1,
            admitted: 1,
            pending: 0,
            retryable: 0,
            indeterminate: 0,
            failedTerminal: 0,
            cancelled: 0,
            deliveryPlans: 1,
            deliverySatisfied: 1,
            deliveryPending: 0,
            deliveryRetryable: 0,
            deliveryExhausted: 0,
            deliveryFailedTerminal: 0,
            deliveryCancelled: 0
          )
        )
    }

    func subscribe(
      bufferCapacity _: Int,
      receive _: @escaping @Sendable (TeraRuntimeChange) async -> Void
    ) -> any TeraRuntimeSubscriptionToken {
        SupportingSubscriptionToken()
    }

    func shutdown() -> TeraRuntimeShutdownReceipt {
        let wasClosed = closed
        closed = true
        return TeraRuntimeShutdownReceipt(state: "closed", alreadyClosed: wasClosed)
    }

    func lastSearchRequest() -> SearchRequest? {
        request
    }

    func settingsRevision() -> UInt64 {
        settings.revision
    }

    private func failure(code: String) -> TeraRuntimeFailure {
        TeraRuntimeFailure(
          schemaVersion: 1,
          code: code,
          category: "invalid_argument",
          retryable: false,
          recoveryActions: [],
          operationID: "test.settings",
          capabilityID: nil,
          safeMessage: "The settings value is invalid."
        )
    }

    private func profileStatus(
      state: TeraOutboxState,
      revision: UInt64,
      settlement: TeraOperationSettlement?
    ) -> TeraProfileStatus {
        TeraProfileStatus(
          id: "profile-operation",
          revision: revision,
          authorPublicKey: String(repeating: "a", count: 64),
          state: state,
          deliveryID: state == .complete ? "delivery" : nil,
          createdAtUnixMilliseconds: 1,
          updatedAtUnixMilliseconds: revision,
          settlement: settlement
        )
    }

    private static func initialSettings() -> TeraMobileSettings {
        TeraMobileSettings(
          revision: 1,
          identity: TeraSettingsIdentityState(
            identities: [
              TeraSettingsIdentity(
                id: "identity",
                publicKeyHex: String(repeating: "a", count: 64)
              ),
            ],
            activeIdentityID: "identity",
            lockState: .unlocked,
            pendingImportOperationID: nil
          ),
          networkEnvironment: .simulator,
          relays: [
            TeraRelayPreference(url: "ws://127.0.0.1:7447", access: .readWrite),
          ],
          blossomAuthority: .loopbackDevelopment,
          blossomPrimaryOrigin: "http://127.0.0.1:3000",
          blossomFallbackOrigins: [],
          allowCellularDownloads: true,
          allowCellularUploads: true,
          allowBackgroundTransfers: true,
          mediaCacheBytes: 256 * 1_048_576,
          mediaCacheArtifacts: 1024
        )
    }

    private func unsupported() -> TeraRuntimeFailure {
        .local(
          operation: "test.supporting",
          code: "test.unsupported",
          safeMessage: "Unsupported test operation."
        )
    }

    private static func profile() -> TeraProfileSummary {
        TeraProfileSummary(
          authorPublicKey: String(repeating: "a", count: 64),
          name: "moss",
          displayName: "Moss Farm",
          about: "Local roots",
          picture: nil,
          banner: nil,
          nip05: "moss@example.com",
          website: "https://moss.example",
          lightningAddress: "moss@example.com"
        )
    }

    private static func card() -> TeraTodayCard {
        TeraTodayCard(
          id: "card",
          type: .foodAvailability,
          sourceEventID: String(repeating: "e", count: 64),
          sourceAddress: nil,
          authorPublicKey: String(repeating: "a", count: 64),
          contractID: "radroots.food_availability.v1",
          title: "Carrots",
          content: "Freshly picked",
          authoredAtUnixSeconds: 1_800_000_000,
          effectiveAtUnixSeconds: 1_800_000_000,
          calendarTiming: nil,
          location: "Metchosin",
          priceAmount: "3",
          priceCurrency: "CAD",
          priceUnit: "lb",
          quantity: "12",
          foodSummary: "Fresh carrots",
          foodPublishedAtUnixSeconds: 1_800_000_000,
          foodStatus: "active",
          contextRank: 1,
          inclusionReason: "local",
          media: [],
          lifecycle: .active,
          rankDigest: nil,
          authorProfile: profile(),
          thread: [],
          localOperationID: nil,
          localOperationState: nil
        )
    }
}

private actor SupportingSubscriptionToken: TeraRuntimeSubscriptionToken {
    func cancel() {}
}
