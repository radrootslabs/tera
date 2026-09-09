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
    private var generation = TeraSessionGeneration.initial

    init(
      runtimeClient: TeraRuntimeClient,
      clock: TeraClock = .system
    ) {
        self.runtimeClient = runtimeClient
        self.clock = clock
    }

    func configure(context: TeraLocalNetwork?) {
        guard self.context != context else { return }
        generation = generation.invalidated()
        self.context = context
        query = ""
        results = []
        state = .idle
    }

    func updateQuery(_ value: String) {
        generation = generation.invalidated()
        query = String(value.prefix(256))
        results = []
        state = .idle
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

        generation = generation.invalidated()
        let requestedGeneration = generation
        state = .loading
        do {
            let loaded = try await runtimeClient.search(
              context: context,
              query: normalized,
              limit: 50,
              asOfUnixSeconds: clock.unixSeconds()
            )
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            results = Self.unique(loaded)
            state = results.isEmpty ? .empty : .loaded
        } catch {
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            results = []
            state = .failed(Self.message(for: error))
        }
    }

    func stop() {
        generation = generation.invalidated()
        query = ""
        results = []
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
    private var generation = TeraSessionGeneration.initial
    private let observation = TeraStoreObservation()

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

    func configure(context: TeraLocalNetwork?) {
        guard self.context != context else { return }
        generation = generation.invalidated()
        self.context = context
        snapshot = nil
        state = .idle
        if observation.isActive {
            observation.stop()
            startObservation()
        }
    }

    func start() async {
        startObservation()
        await reload()
    }

    func reload() async {
        guard let context else {
            snapshot = nil
            state = .failed("Choose a local network before loading your profile.")
            return
        }
        generation = generation.invalidated()
        let requestedGeneration = generation
        if snapshot == nil {
            state = .loading
        }
        do {
            let loaded = try await runtimeClient.me(
              context: context,
              asOfUnixSeconds: clock.unixSeconds()
            )
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            snapshot = loaded
            state = loaded.cards.isEmpty && loaded.profile == nil ? .empty : .loaded
        } catch {
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            state = .failed(Self.message(for: error))
        }
    }

    func stop() {
        generation = generation.invalidated()
        observation.stop()
        observationState = .stopped
        snapshot = nil
        state = .idle
    }

    private func startObservation() {
        observation.start(
          client: runtimeClient, capacity: 8, delay: observationDelay,
          state: { [weak self] in self?.observationState = $0 },
          change: { [weak self] change in
                switch change.kind {
                case .today, .identity, .profile, .media, .drafts:
                    await self?.reload()
                case .initial, .settings, .relay, .lifecycle:
                    break
                }
            }
        )
    }

    private static func message(for error: Error) -> String {
        TeraUserMessages.text(for: error, fallback: .profileUnavailable)
    }
}
