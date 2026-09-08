import Foundation

enum TeraSupportingLoadState: Sendable, Equatable {
    case idle
    case loading
    case loaded
    case empty
    case failed(String)
}

@MainActor
final class TeraSearchStore: ObservableObject {
    @Published private(set) var query = ""
    @Published private(set) var results: [TeraSearchResult] = []
    @Published private(set) var state: TeraSupportingLoadState = .idle

    private let runtimeClient: TeraRuntimeClient
    private let clock: TeraClock
    private var context: TeraLocalNetwork?
    private var generation: UInt64 = 0

    init(
      runtimeClient: TeraRuntimeClient,
      clock: TeraClock = .system
    ) {
        self.runtimeClient = runtimeClient
        self.clock = clock
    }

    func configure(context: TeraLocalNetwork?) {
        guard self.context != context else { return }
        generation &+= 1
        self.context = context
        results = []
        state = .idle
    }

    func updateQuery(_ value: String) {
        query = String(value.prefix(256))
        if query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            generation &+= 1
            results = []
            state = .idle
        }
    }

    func search() async {
        guard let context else {
            state = .failed("Choose a local network before searching.")
            return
        }
        let normalized = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty,
              normalized.utf8.count <= 256,
              !normalized.contains(where: \.isNewline),
              !normalized.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
        else {
            results = []
            state = .idle
            return
        }

        generation &+= 1
        let requestedGeneration = generation
        state = .loading
        do {
            let loaded = try await runtimeClient.search(
              context: context,
              query: normalized,
              limit: 50,
              asOfUnixSeconds: clock.unixSeconds()
            )
            guard requestedGeneration == generation, !Task.isCancelled else { return }
            results = Self.unique(loaded)
            state = results.isEmpty ? .empty : .loaded
        } catch {
            guard requestedGeneration == generation, !Task.isCancelled else { return }
            results = []
            state = .failed(Self.message(for: error))
        }
    }

    func stop() {
        generation &+= 1
        state = .idle
    }

    private static func unique(_ values: [TeraSearchResult]) -> [TeraSearchResult] {
        var identifiers = Set<String>()
        return values.filter { identifiers.insert("\($0.type):\($0.id)").inserted }
    }

    private static func message(for error: Error) -> String {
        TeraUserMessages.text(for: error, fallback: .searchUnavailable)
    }
}

@MainActor
final class TeraMeStore: ObservableObject {
    @Published private(set) var snapshot: TeraMeSnapshot?
    @Published private(set) var state: TeraSupportingLoadState = .idle
    @Published private(set) var observationState: TeraRuntimeObservationState = .inactive

    private let runtimeClient: TeraRuntimeClient
    private let clock: TeraClock
    private let observationDelay: @Sendable (UInt32) async throws -> Void
    private var context: TeraLocalNetwork?
    private var generation: UInt64 = 0
    private var observationTask: Task<Void, Never>?
    private var isStarted = false
    private var observationGeneration: UInt64 = 0

    init(
      runtimeClient: TeraRuntimeClient,
      clock: TeraClock = .system,
      observationDelay: @escaping @Sendable (UInt32) async throws -> Void =
            TeraRuntimeObservationBackoff.sleep
    ) {
        self.runtimeClient = runtimeClient
        self.clock = clock
        self.observationDelay = observationDelay
    }

    deinit {
        observationTask?.cancel()
    }

    func configure(context: TeraLocalNetwork?) {
        guard self.context != context else { return }
        generation &+= 1
        self.context = context
        snapshot = nil
        state = .idle
    }

    func start() async {
        if !isStarted {
            isStarted = true
            observationGeneration &+= 1
            let requestedGeneration = observationGeneration
            observationTask = Task { [weak self] in
                await self?.observe(generation: requestedGeneration)
            }
        }
        await reload()
    }

    func reload() async {
        guard let context else {
            snapshot = nil
            state = .failed("Choose a local network before loading your profile.")
            return
        }
        generation &+= 1
        let requestedGeneration = generation
        if snapshot == nil {
            state = .loading
        }
        do {
            let loaded = try await runtimeClient.me(
              context: context,
              asOfUnixSeconds: clock.unixSeconds()
            )
            guard requestedGeneration == generation, !Task.isCancelled else { return }
            snapshot = loaded
            state = loaded.cards.isEmpty && loaded.profile == nil ? .empty : .loaded
        } catch {
            guard requestedGeneration == generation, !Task.isCancelled else { return }
            state = .failed(Self.message(for: error))
        }
    }

    func stop() {
        generation &+= 1
        isStarted = false
        observationGeneration &+= 1
        observationTask?.cancel()
        observationTask = nil
        observationState = .stopped
    }

    private func observe(generation: UInt64) async {
        var attempt: UInt32 = 0
        while isStarted, observationGeneration == generation, !Task.isCancelled {
            observationState = .subscribing(attempt: attempt &+ 1)
            var failureMessage = TeraUserMessages.text(.runtimeObservationUnavailable)
            do {
                let changes = try await runtimeClient.changes(bufferCapacity: 8)
                observationState = .active
                for await change in changes {
                    guard !Task.isCancelled else { break }
                    attempt = 0
                    switch change.kind {
                    case .today, .identity, .profile, .media, .drafts:
                        await reload()
                    case .initial, .settings, .relay, .lifecycle:
                        continue
                    }
                }
            } catch {
                failureMessage = TeraUserMessages.text(
                  for: error,
                  fallback: .runtimeObservationUnavailable
                )
            }
            guard isStarted, observationGeneration == generation, !Task.isCancelled else { break }
            attempt = attempt == .max ? .max : attempt + 1
            observationState = .retrying(attempt: attempt, message: failureMessage)
            do {
                try await observationDelay(attempt)
            } catch {
                break
            }
        }
        observationDidFinish(generation: generation)
    }

    private func observationDidFinish(generation: UInt64) {
        guard observationGeneration == generation else { return }
        observationTask = nil
        isStarted = false
        if case .retrying = observationState {
            return
        }
        observationState = .stopped
    }

    private static func message(for error: Error) -> String {
        TeraUserMessages.text(for: error, fallback: .profileUnavailable)
    }
}

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
    private var generation: UInt64 = 0

    init(runtimeClient: TeraRuntimeClient) {
        self.runtimeClient = runtimeClient
    }

    func load(profile: TeraProfileSummary?) async {
        generation &+= 1
        let requestedGeneration = generation
        isWorking = true
        defer {
            if requestedGeneration == generation {
                isWorking = false
            }
        }
        do {
            let loaded = try await runtimeClient.mobileSettings()
            guard requestedGeneration == generation, !Task.isCancelled else { return }
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
            guard requestedGeneration == generation, !Task.isCancelled else { return }
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
        guard let settings else { return false }
        generation &+= 1
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
            guard requestedGeneration == generation, !Task.isCancelled else { return false }
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
            guard requestedGeneration == generation, !Task.isCancelled else { return false }
            record(error)
            return false
        }
    }

    func saveProfile() async {
        generation &+= 1
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
            guard requestedGeneration == generation, !Task.isCancelled else { return }
            profileStatus = status
            message = "Profile update saved to the durable outbox."
            failureCode = nil
        } catch {
            guard requestedGeneration == generation, !Task.isCancelled else { return }
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
        generation &+= 1
        isWorking = false
    }

    private func runProfileOperation(
        _ operation: @escaping () async throws -> TeraProfileStatus
    ) async {
        generation &+= 1
        let requestedGeneration = generation
        isWorking = true
        defer {
            if requestedGeneration == generation {
                isWorking = false
            }
        }
        do {
            let status = try await operation()
            guard requestedGeneration == generation, !Task.isCancelled else { return }
            profileStatus = status
            message = status.honestSummary
            failureCode = nil
        } catch {
            guard requestedGeneration == generation, !Task.isCancelled else { return }
            record(error)
        }
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
