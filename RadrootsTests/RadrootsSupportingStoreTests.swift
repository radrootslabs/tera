@testable import RadrootsApp
import XCTest

final class RadrootsSupportingStoreTests: XCTestCase {
    @MainActor
    func testSearchUsesCurrentContextDeduplicatesAndClearsEmptyQueries() async throws {
        let backend = SupportingBackend()
        let client = try await Self.startedClient(backend)
        let store = RadrootsSearchStore(runtimeClient: client, now: { 1_800_000_000 })
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
        let store = RadrootsSearchStore(runtimeClient: client, now: { 1_800_000_000 })
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
        let store = RadrootsMeStore(runtimeClient: client, now: { 1_800_000_000 })
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
        let first = RadrootsStableVisualIdentity(publicKeyHex: String(repeating: "a", count: 64))
        let repeated = RadrootsStableVisualIdentity(publicKeyHex: String(repeating: "a", count: 64))
        let second = RadrootsStableVisualIdentity(publicKeyHex: String(repeating: "b", count: 64))

        XCTAssertEqual(first, repeated)
        XCTAssertNotEqual(first.digestHex, second.digestHex)
        XCTAssertTrue((0 ..< 12).contains(first.paletteIndex))
    }

    @MainActor
    func testSettingsRoundTripUsesTypedRuntimeAndReportsReconfigurationEffects() async throws {
        let backend = SupportingBackend()
        let client = try await Self.startedClient(backend)
        let store = RadrootsSettingsStore(runtimeClient: client)

        await store.load(profile: nil)
        XCTAssertEqual(store.settings?.identity.identities.count, 1)
        store.networkEnvironment = .physicalDevice
        store.relays = [
          RadrootsRelayPreference(url: "wss://radroots.org/", access: .readWrite),
          RadrootsRelayPreference(url: "wss://read.example/", access: .readOnly),
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
        let store = RadrootsSettingsStore(runtimeClient: client)
        await store.load(profile: nil)
        store.relays = [RadrootsRelayPreference(url: "https://not-a-relay.example", access: .readWrite)]

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
        let store = RadrootsSettingsStore(runtimeClient: client)
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

    private static func startedClient(_ backend: SupportingBackend) async throws -> RadrootsRuntimeClient {
        let client = RadrootsRuntimeClient { _ in
            await RadrootsRuntimeBackendStart(backend: backend, snapshot: backend.snapshotValue())
        }
        _ = try await client.start(configuration: configuration())
        return client
    }

    private static func configuration() -> RadrootsRuntimeLaunchConfiguration {
        RadrootsRuntimeLaunchConfiguration(
          applicationSupportDirectory: "/tmp/radroots-supporting-tests",
          publicKeyHex: String(repeating: "a", count: 64),
          sourceGenerationHex: String(repeating: "c", count: 64),
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

    private static func context(id: String) -> RadrootsLocalNetwork {
        RadrootsLocalNetwork(
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

private struct SupportingSigner: RadrootsRuntimeSigner {
    func availability() -> RadrootsRuntimeSignerAvailability {
        .ready
    }

    func sign(_: RadrootsRuntimeSigningRequest) -> RadrootsRuntimeSigningOutcome {
        .failed
    }
}

private actor SupportingBackend: RadrootsRuntimeBackend {
    struct SearchRequest: Sendable {
        let contextID: String
        let query: String
    }

    private let searchDelayNanoseconds: UInt64
    private var request: SearchRequest?
    private var closed = false
    private var settings = SupportingBackend.initialSettings()
    private var savedProfile: RadrootsProfileMetadataInput?

    init(searchDelayNanoseconds: UInt64 = 0) {
        self.searchDelayNanoseconds = searchDelayNanoseconds
    }

    func snapshotValue() -> RadrootsRuntimeSnapshot {
        RadrootsRuntimeSnapshot(
          identity: RadrootsRuntimeIdentity(
            publicKeyHex: String(repeating: "a", count: 64),
            hostSignerConfigured: true
          ),
          relay: nil,
          blossomConfiguration: nil,
          blossomEvidence: nil,
          crateName: "radroots_mobile_ffi",
          crateVersion: "0.1.0-alpha",
          isClosed: closed
        )
    }

    func snapshot() -> RadrootsRuntimeSnapshot {
        snapshotValue()
    }

    func todayPage(request _: RadrootsTodayPageRequest) throws -> RadrootsTodayPage {
        throw unsupported()
    }

    func refreshToday(
      context _: RadrootsLocalNetwork,
      nowUnixSeconds _: UInt64,
      update _: RadrootsTodayProjectionUpdate
    ) throws -> RadrootsTodayRefreshReceipt {
        throw unsupported()
    }

    func search(
      context: RadrootsLocalNetwork,
      query: String,
      limit _: UInt16,
      asOfUnixSeconds _: UInt64
    ) async throws -> [RadrootsSearchResult] {
        request = SearchRequest(contextID: context.id, query: query)
        if searchDelayNanoseconds > 0 {
            try await Task.sleep(nanoseconds: searchDelayNanoseconds)
        }
        let card = Self.card()
        let profile = Self.profile()
        return [
          RadrootsSearchResult(type: .card, id: "card", card: card, profile: nil),
          RadrootsSearchResult(type: .card, id: "card", card: card, profile: nil),
          RadrootsSearchResult(type: .profile, id: "profile", card: nil, profile: profile),
        ]
    }

    func me(
      context _: RadrootsLocalNetwork,
      asOfUnixSeconds _: UInt64
    ) -> RadrootsMeSnapshot {
        RadrootsMeSnapshot(
          publicKey: String(repeating: "a", count: 64),
          profile: Self.profile(),
          cards: [Self.card()]
        )
    }

    func mobileSettings() -> RadrootsMobileSettings {
        settings
    }

    func replaceMobileSettings(
        input: RadrootsReplaceSettings
    ) throws -> RadrootsSettingsTransition {
        guard input.expectedRevision == settings.revision else {
            throw failure(code: "settings_revision_conflict")
        }
        guard input.relays.allSatisfy({ $0.url.hasPrefix("ws://") || $0.url.hasPrefix("wss://") }) else {
            throw failure(code: "invalid_relay_endpoint")
        }
        settings = RadrootsMobileSettings(
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
        return RadrootsSettingsTransition(
          settings: settings,
          runtimeRestartRequired: true,
          outboxRequeueRequired: true,
          mediaCacheInvalidationRequired: true
        )
    }

    func saveProfileMetadata(input: RadrootsProfileMetadataInput) -> RadrootsProfileStatus {
        savedProfile = input
        return profileStatus(state: .queued, revision: 1, settlement: nil)
    }

    func advanceProfile(operationID _: String) -> RadrootsProfileStatus {
        profileStatus(
          state: .complete,
          revision: 2,
          settlement: RadrootsOperationSettlement(
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
      receive _: @escaping @Sendable (RadrootsRuntimeChange) async -> Void
    ) -> any RadrootsRuntimeSubscriptionToken {
        SupportingSubscriptionToken()
    }

    func shutdown() -> RadrootsRuntimeShutdownReceipt {
        let wasClosed = closed
        closed = true
        return RadrootsRuntimeShutdownReceipt(state: "closed", alreadyClosed: wasClosed)
    }

    func lastSearchRequest() -> SearchRequest? {
        request
    }

    func settingsRevision() -> UInt64 {
        settings.revision
    }

    private func failure(code: String) -> RadrootsRuntimeFailure {
        RadrootsRuntimeFailure(
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
      state: RadrootsOutboxState,
      revision: UInt64,
      settlement: RadrootsOperationSettlement?
    ) -> RadrootsProfileStatus {
        RadrootsProfileStatus(
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

    private static func initialSettings() -> RadrootsMobileSettings {
        RadrootsMobileSettings(
          revision: 1,
          identity: RadrootsSettingsIdentityState(
            identities: [
              RadrootsSettingsIdentity(
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
            RadrootsRelayPreference(url: "ws://127.0.0.1:7447", access: .readWrite),
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

    private func unsupported() -> RadrootsRuntimeFailure {
        .local(
          operation: "test.supporting",
          code: "test.unsupported",
          safeMessage: "Unsupported test operation."
        )
    }

    private static func profile() -> RadrootsProfileSummary {
        RadrootsProfileSummary(
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

    private static func card() -> RadrootsTodayCard {
        RadrootsTodayCard(
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
          eventStartUnixSeconds: nil,
          eventEndUnixSeconds: nil,
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

private actor SupportingSubscriptionToken: RadrootsRuntimeSubscriptionToken {
    func cancel() {}
}
