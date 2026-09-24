import Foundation
@testable import TeraApp

actor SupportingBackend: TeraRuntimeBackend {
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
}

extension SupportingBackend {
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
