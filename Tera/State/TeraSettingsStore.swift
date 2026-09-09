import Foundation

@MainActor
final class TeraSettingsStore: ObservableObject {
    @Published private(set) var settings: TeraMobileSettings?
    @Published private(set) var profileStatus: TeraProfileStatus?
    @Published private(set) var isWorking = false
    @Published private(set) var message: String?
    @Published private(set) var failureCode: String?

    @Published var networkEnvironment: TeraSettingsNetworkEnvironment = .publicNetwork
    @Published var relays: [TeraRelayPreference] = []
    @Published var blossomAuthority: TeraBlossomAuthorityPreference = .publicWebPKI
    @Published var blossomPrimaryOrigin = ""
    @Published var blossomFallbackOrigins = ""
    @Published var allowCellularDownloads = true
    @Published var allowCellularUploads = true
    @Published var allowBackgroundTransfers = true
    @Published var mediaCacheMegabytes = 256
    @Published var mediaCacheArtifacts = 1024
    @Published var profileName = ""
    @Published var profileDisplayName = ""
    @Published var profileAbout = ""
    @Published var profileNip05 = ""
    @Published var profileBot = false

    private let runtimeClient: TeraRuntimeClient
    private var generation = TeraSessionGeneration.initial
    /// Presentation invalidation cannot release an active mutation's admission.
    private var mutationInProgress = false
    private var configuration: TeraPresentationConfiguration?

    init(runtimeClient: TeraRuntimeClient) {
        self.runtimeClient = runtimeClient
    }

    func load(profile: TeraProfileSummary?) async {
        generation = generation.invalidated()
        let requestedGeneration = generation
        isWorking = true
        defer {
            if requestedGeneration == generation {
                isWorking = false
            }
        }
        do {
            let loaded = try await runtimeClient.mobileSettings()
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            apply(loaded)
            if let profile {
                profileName = profile.name ?? ""
                profileDisplayName = profile.displayName ?? ""
                profileAbout = profile.about ?? ""
                profileNip05 = profile.nip05 ?? ""
            }
            message = nil
            failureCode = nil
        } catch {
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            record(error)
        }
    }

    func addRelay() {
        relays.append(TeraRelayPreference(url: "", access: .readWrite))
    }

    func removeRelays(at offsets: IndexSet) {
        relays.remove(atOffsets: offsets)
    }

    func saveSettings() async -> Bool {
        guard let settings, reserveMutation() else { return false }
        defer { mutationInProgress = false }
        generation = generation.invalidated()
        let requestedGeneration = generation
        isWorking = true
        defer {
            if requestedGeneration == generation {
                isWorking = false
            }
        }
        let fallbacks = blossomFallbackOrigins
            .components(separatedBy: CharacterSet(charactersIn: ",;\n\r"))
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        do {
            let transition = try await runtimeClient.replaceMobileSettings(
                input: TeraReplaceSettings(
                  expectedRevision: settings.revision,
                  networkEnvironment: networkEnvironment,
                  relays: relays,
                  blossomAuthority: blossomAuthority,
                  blossomPrimaryOrigin: blossomPrimaryOrigin,
                  blossomFallbackOrigins: fallbacks,
                  allowCellularDownloads: allowCellularDownloads,
                  allowCellularUploads: allowCellularUploads,
                  allowBackgroundTransfers: allowBackgroundTransfers,
                  mediaCacheBytes: UInt64(max(mediaCacheMegabytes, 1)) * 1_048_576,
                  mediaCacheArtifacts: UInt32(max(mediaCacheArtifacts, 1))
                )
            )
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return false }
            apply(transition.settings)
            let effects = [
              transition.runtimeRestartRequired ? "runtime restart" : nil,
              transition.outboxRequeueRequired ? "outbox requeue" : nil,
              transition.mediaCacheInvalidationRequired ? "media cache refresh" : nil,
            ].compactMap(\.self)
            message = effects.isEmpty
                ? "Settings saved."
                : "Settings saved; required changes: \(effects.joined(separator: ", "))."
            failureCode = nil
            return transition.runtimeRestartRequired
        } catch {
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return false }
            record(error)
            return false
        }
    }

    func saveProfile() async {
        guard reserveMutation() else { return }
        defer { mutationInProgress = false }
        generation = generation.invalidated()
        let requestedGeneration = generation
        isWorking = true
        defer {
            if requestedGeneration == generation {
                isWorking = false
            }
        }
        do {
            let status = try await runtimeClient.saveProfileMetadata(
                input: TeraProfileMetadataInput(
                  name: profileName,
                  displayName: optional(profileDisplayName),
                  about: optional(profileAbout),
                  picture: nil,
                  banner: nil,
                  nip05: optional(profileNip05),
                  bot: profileBot
                )
            )
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            profileStatus = status
            message = "Profile update saved to the durable outbox."
            failureCode = nil
        } catch {
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            record(error)
        }
    }

    func advanceProfile() async {
        guard let profileStatus else { return }
        await runProfileOperation {
            try await self.runtimeClient.advanceProfile(operationID: profileStatus.id)
        }
    }

    func cancelProfile() async {
        guard let profileStatus else { return }
        await runProfileOperation {
            try await self.runtimeClient.cancelProfile(
              operationID: profileStatus.id,
              expectedRevision: profileStatus.revision
            )
        }
    }

    func stop() {
        generation = generation.invalidated()
        isWorking = false
    }

    func configure(snapshot: TeraRuntimeSnapshot) {
        let updated = TeraPresentationConfiguration(snapshot: snapshot)
        guard configuration != updated else { return }
        configuration = updated
        stop()
        settings = nil
        profileStatus = nil
        message = nil
        failureCode = nil
        networkEnvironment = .publicNetwork
        relays = []
        blossomAuthority = .publicWebPKI
        blossomPrimaryOrigin = ""
        blossomFallbackOrigins = ""
        allowCellularDownloads = true
        allowCellularUploads = true
        allowBackgroundTransfers = true
        mediaCacheMegabytes = 256
        mediaCacheArtifacts = 1024
        profileName = ""
        profileDisplayName = ""
        profileAbout = ""
        profileNip05 = ""
        profileBot = false
    }

    private func runProfileOperation(
        _ operation: @escaping () async throws -> TeraProfileStatus
    ) async {
        guard reserveMutation() else { return }
        defer { mutationInProgress = false }
        generation = generation.invalidated()
        let requestedGeneration = generation
        isWorking = true
        defer {
            if requestedGeneration == generation {
                isWorking = false
            }
        }
        do {
            let status = try await operation()
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            profileStatus = status
            message = status.honestSummary
            failureCode = nil
        } catch {
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            record(error)
        }
    }

    private func reserveMutation() -> Bool {
        guard !mutationInProgress, !Task.isCancelled else { return false }
        mutationInProgress = true
        return true
    }

    private func apply(_ loaded: TeraMobileSettings) {
        settings = loaded
        networkEnvironment = loaded.networkEnvironment
        relays = loaded.relays
        blossomAuthority = loaded.blossomAuthority
        blossomPrimaryOrigin = loaded.blossomPrimaryOrigin
        blossomFallbackOrigins = loaded.blossomFallbackOrigins.joined(separator: "\n")
        allowCellularDownloads = loaded.allowCellularDownloads
        allowCellularUploads = loaded.allowCellularUploads
        allowBackgroundTransfers = loaded.allowBackgroundTransfers
        mediaCacheMegabytes = Int(loaded.mediaCacheBytes / 1_048_576)
        mediaCacheArtifacts = Int(loaded.mediaCacheArtifacts)
    }

    private func record(_ error: Error) {
        if case let TeraRuntimeClientError.support(failure) = error {
            failureCode = failure.code
        } else {
            failureCode = "ios.settings.operation_failed"
        }
        message = TeraUserMessages.text(for: error, fallback: .settingsOperationFailed)
    }

    private func optional(_ value: String) -> String? {
        let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return normalized.isEmpty ? nil : normalized
    }
}
