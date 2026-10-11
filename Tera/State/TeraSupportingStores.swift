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
    private static let rawQueryBytesMax = 256
    private static let normalizedQueryBytesMax = 256

    @Published private(set) var query = ""
    @Published private(set) var results: [TeraSearchResult] = []
    @Published private(set) var state: TeraSupportingLoadState = .idle
    @Published private(set) var observationState: TeraRuntimeObservationState = .inactive
    var mediaWillChange: ([TeraMediaReference], [TeraMediaReference], TeraLocalNetwork?) -> Void = { _, _, _ in }

    private let runtimeClient: TeraRuntimeClient
    private let clock: TeraClock
    private let observationDelay: @Sendable (UInt32) async throws -> Void
    private var context: TeraLocalNetwork?
    private var generation = TeraSessionGeneration.initial
    private let observation = TeraStoreObservation()

    init(
      runtimeClient: TeraRuntimeClient,
      clock: TeraClock = .system,
      observationDelay: @escaping @Sendable (UInt32) async throws -> Void = TeraRuntimeObservationBackoff.sleep
    ) {
        self.runtimeClient = runtimeClient
        self.clock = clock
        self.observationDelay = observationDelay
    }

    func configure(context: TeraLocalNetwork?) {
        guard self.context != context else { return }
        generation = generation.invalidated()
        replaceResults([])
        self.context = context
        query = ""
        state = .idle
        if observation.isActive {
            observation.stop()
            start()
        }
    }

    func updateQuery(_ value: String) {
        generation = generation.invalidated()
        query = value
        replaceResults([])
        state = .idle
    }

    func start() {
        observation.start(
          client: runtimeClient, buffer: (capacity: 8, delay: observationDelay),
          state: { [weak self] in self?.observationState = $0 },
          accepts: { [weak self] in $0.matches(context: self?.context) },
          invalidate: { [weak self] batch in
              guard batch.contains(anyOf: [.today, .identity, .profile]) else { return }
              self?.invalidatePresentation()
          },
          refresh: { [weak self] batch in
              guard batch.contains(anyOf: [.today, .identity, .profile]) else { return }
              await self?.search()
          }
        )
    }

    func invalidatePresentation() {
        generation = generation.invalidated()
        replaceResults([])
        state = .idle
    }

    func currentResult(id: String, type: TeraSearchResultType, context: TeraLocalNetwork?) -> TeraSearchResult? {
        guard self.context == context else { return nil }
        return results.first { $0.id == id && $0.type == type }
    }

    func currentMedia(context: TeraLocalNetwork?) -> [TeraMediaReference] {
        guard self.context == context else { return [] }
        return results.flatMap { supportingMedia(card: $0.card, profile: $0.profile) }
    }

    func search() async {
        guard let context else {
            state = .failed("Choose a local network before searching.")
            return
        }
        guard query.utf8.count <= Self.rawQueryBytesMax,
              !query.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
        else {
            replaceResults([])
            state = .idle
            return
        }
        let normalized = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty,
              normalized.lowercased().utf8.count <= Self.normalizedQueryBytesMax
        else {
            replaceResults([])
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
            replaceResults(Self.unique(loaded))
            state = results.isEmpty ? .empty : .loaded
        } catch {
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            replaceResults([])
            state = .failed(Self.message(for: error))
        }
    }

    func stop() {
        generation = generation.invalidated()
        observation.stop()
        observationState = .stopped
        query = ""
        replaceResults([])
        state = .idle
    }

    private func replaceResults(_ values: [TeraSearchResult]) {
        mediaWillChange(
          results.flatMap { supportingMedia(card: $0.card, profile: $0.profile) },
          values.flatMap { supportingMedia(card: $0.card, profile: $0.profile) }, context
        )
        results = values
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
    var mediaWillChange: ([TeraMediaReference], [TeraMediaReference], TeraLocalNetwork?) -> Void = { _, _, _ in }

    private let runtimeClient: TeraRuntimeClient
    private let clock: TeraClock
    private let observationDelay: @Sendable (UInt32) async throws -> Void
    private var context: TeraLocalNetwork?
    private var generation = TeraSessionGeneration.initial
    private let observation = TeraStoreObservation()
    private var reloadTask: Task<Void, Never>?

    deinit { reloadTask?.cancel() }

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
        reloadTask?.cancel()
        reloadTask = nil
        replaceSnapshot(nil)
        self.context = context
        state = .idle
        if observation.isActive {
            observation.stop()
            startObservation()
        }
    }

    func start() async {
        startObservation()
        reloadTask?.cancel()
        reloadTask = Task { [weak self] in await self?.reload() }
        let task = reloadTask
        await withTaskCancellationHandler { await task?.value } onCancel: { task?.cancel() }
        guard !Task.isCancelled else { return }
        await observation.waitForRefresh()
    }

    func reload() async {
        guard let context else {
            replaceSnapshot(nil)
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
            replaceSnapshot(loaded)
            state = loaded.cards.isEmpty && loaded.profile == nil ? .empty : .loaded
        } catch {
            guard requestedGeneration == generation, generation.isActive, !Task.isCancelled else { return }
            if Self.refusesCurrentScope(error) {
                replaceSnapshot(nil)
            }
            state = .failed(Self.message(for: error))
        }
    }

    func stop() {
        generation = generation.invalidated()
        observation.stop()
        reloadTask?.cancel()
        reloadTask = nil
        observationState = .stopped
        replaceSnapshot(nil)
        state = .idle
    }

    func invalidatePresentation() {
        generation = generation.invalidated()
        reloadTask?.cancel()
        replaceSnapshot(nil)
        state = .idle
    }

    func currentCard(id: String, context: TeraLocalNetwork?) -> TeraTodayCard? {
        guard self.context == context else { return nil }
        return snapshot?.cards.first { $0.id == id }
    }

    func currentProfile(context: TeraLocalNetwork?) -> TeraProfileSummary? {
        self.context == context ? snapshot?.profile : nil
    }

    func currentMedia(context: TeraLocalNetwork?) -> [TeraMediaReference] {
        self.context == context ? Self.references(snapshot) : []
    }

    private func replaceSnapshot(_ value: TeraMeSnapshot?) {
        mediaWillChange(Self.references(snapshot), Self.references(value), context)
        snapshot = value
    }

    private static func references(_ snapshot: TeraMeSnapshot?) -> [TeraMediaReference] {
        (snapshot?.cards ?? []).flatMap { supportingMedia(card: $0, profile: nil) }
          + supportingMedia(card: nil, profile: snapshot?.profile)
    }

    private func startObservation() {
        observation.start(
          client: runtimeClient, buffer: (capacity: 8, delay: observationDelay),
          state: { [weak self] in self?.observationState = $0 },
          accepts: { [weak self] in $0.matches(context: self?.context) },
          invalidate: { [weak self] batch in
                guard batch.contains(anyOf: [.today, .identity, .profile, .drafts]) else { return }
                self?.invalidatePresentation()
            },
          refresh: { [weak self] batch in
                guard batch.contains(anyOf: [.today, .identity, .profile, .drafts]) else { return }
                await self?.reloadTask?.value
                guard !Task.isCancelled else { return }
                await self?.reload()
            }
        )
    }

    private static func refusesCurrentScope(_ error: Error) -> Bool {
        switch TeraRuntimeClient.failure(from: error, operation: "me").code {
        case "today_invalid_request", "today_refresh_required", "today_projection_rebuilding",
             "today_cursor_invalid", "today_reader_unsupported", "today_runtime_unavailable", "today_state_failed",
             "storage_space_insufficient":
            true
        default:
            false
        }
    }

    private static func message(for error: Error) -> String {
        TeraUserMessages.text(for: error, fallback: .profileUnavailable)
    }
}

private func supportingMedia(card: TeraTodayCard?, profile: TeraProfileSummary?) -> [TeraMediaReference] {
    func references(_ value: TeraProfileSummary?) -> [TeraMediaReference] {
        [value?.picture, value?.banner].compactMap(\.self)
    }
    return (card?.media ?? []) + references(card?.authorProfile) + references(profile)
      + (card?.thread ?? []).flatMap { references($0.authorProfile) }
}
