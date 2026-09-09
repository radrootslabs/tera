import Foundation

enum TeraTodayLoadState: Sendable, Equatable {
    case idle
    case loading
    case loaded
    case empty
    case offline(message: String)
    case failed(message: String)
}

@MainActor
final class TeraTodayStore: ObservableObject {
    @Published private(set) var contexts: [TeraLocalNetwork]
    @Published private(set) var selectedContextID: String?
    @Published private(set) var cards: [TeraTodayCard] = []
    @Published private(set) var state: TeraTodayLoadState = .idle
    @Published private(set) var isLoadingNextPage = false
    @Published private(set) var observationState: TeraRuntimeObservationState = .inactive

    private let runtimeClient: TeraRuntimeClient
    private let pageSize: UInt16
    private let clock: TeraClock
    private let observationDelay: @Sendable (UInt32) async throws -> Void
    private var frozenAsOfUnixSeconds: UInt64?
    private var nextCursor: String?
    private var requestGeneration = TeraSessionGeneration.initial
    private let observation = TeraStoreObservation()
    private var configuration: TeraPresentationConfiguration?
    private var reloadTask: Task<Void, Never>?

    init(
      runtimeClient: TeraRuntimeClient,
      contexts: [TeraLocalNetwork] = [],
      selectedContextID: String? = nil,
      pageSize: UInt16 = 20,
      clock: TeraClock = .system,
      observationDelay: @escaping @Sendable (UInt32) async throws -> Void =
            TeraRuntimeObservationBackoff.sleep
    ) {
        self.runtimeClient = runtimeClient
        self.contexts = Self.unique(contexts)
        self.pageSize = min(max(pageSize, 1), 100)
        self.clock = clock
        self.observationDelay = observationDelay
        if let selectedContextID,
           self.contexts.contains(where: { $0.id == selectedContextID })
        {
            self.selectedContextID = selectedContextID
        } else {
            self.selectedContextID = self.contexts.first?.id
        }
    }

    deinit {
        reloadTask?.cancel()
    }

    var selectedContext: TeraLocalNetwork? {
        contexts.first(where: { $0.id == selectedContextID })
    }

    var canLoadNextPage: Bool {
        nextCursor != nil && !isLoadingNextPage
    }

    func configure(snapshot: TeraRuntimeSnapshot) {
        let updated = TeraPresentationConfiguration(snapshot: snapshot)
        guard configuration != updated else { return }
        let previous = configuration
        let reload = observation.isActive
        invalidatePresentation()
        configuration = updated
        if previous != nil || contexts.isEmpty {
            contexts = [updated.context]
            selectedContextID = updated.context.id
        }
        if reload {
          scheduleReload()
        }
    }

    func start() async {
        guard !observation.isActive, !Task.isCancelled else { return }
        startObservation()
        await reload()
    }

    func stop() {
        observation.stop()
        observationState = .stopped
        requestGeneration = requestGeneration.invalidated()
        reloadTask?.cancel()
        reloadTask = nil
        isLoadingNextPage = false
    }

    private func invalidatePresentation() {
        requestGeneration = requestGeneration.invalidated()
        reloadTask?.cancel()
        reloadTask = nil
        cards = []
        frozenAsOfUnixSeconds = nil
        nextCursor = nil
        isLoadingNextPage = false
        state = .idle
        if observation.isActive {
            observation.stop()
            startObservation()
        }
    }

    func selectContext(id: String) {
        guard id != selectedContextID,
              contexts.contains(where: { $0.id == id })
        else {
            return
        }
        invalidatePresentation()
        selectedContextID = id
        scheduleReload()
    }

    func replaceContexts(_ updatedContexts: [TeraLocalNetwork], selectedID: String?) {
        let updatedContexts = Self.unique(updatedContexts)
        invalidatePresentation()
        contexts = updatedContexts
        selectedContextID =
            selectedID.flatMap { requested in
                updatedContexts.contains(where: { $0.id == requested }) ? requested : nil
            } ?? updatedContexts.first?.id
        scheduleReload()
    }

    private func scheduleReload() {
        reloadTask = Task { [weak self] in await self?.reload() }
    }

    func reload(
      refreshProjection: Bool = true,
      update: TeraTodayProjectionUpdate = .incremental
    ) async {
        guard let context = selectedContext else {
            cards = []
            state = .failed(message: "Choose a local network to load Today.")
            return
        }

        requestGeneration = requestGeneration.invalidated()
        let generation = requestGeneration
        frozenAsOfUnixSeconds = nil
        nextCursor = nil
        isLoadingNextPage = false
        if cards.isEmpty {
            state = .loading
        }

        var refreshFailure: Error?
        if refreshProjection {
            do {
                _ = try await runtimeClient.refreshToday(
                  context: context,
                  nowUnixSeconds: clock.unixSeconds(),
                  update: update
                )
            } catch {
                refreshFailure = error
            }
        }

        guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
        do {
            let asOf = try clock.unixSeconds()
            let page = try await runtimeClient.todayPage(
                request: .first(
                  context: context,
                  limit: pageSize,
                  asOfUnixSeconds: asOf
                )
            )
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
            frozenAsOfUnixSeconds = page.asOfUnixSeconds
            nextCursor = page.nextCursor
            cards = Self.unique(page.items)
            state = refreshFailure.map(Self.failureState) ?? (cards.isEmpty ? .empty : .loaded)
        } catch {
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
            state = Self.failureState(error)
        }
    }

    func loadNextPage() async {
        guard let context = selectedContext,
              let cursor = nextCursor,
              !isLoadingNextPage
        else {
            return
        }
        let generation = requestGeneration
        isLoadingNextPage = true
        defer {
            if generation == requestGeneration {
                isLoadingNextPage = false
            }
        }

        do {
            let page = try await runtimeClient.todayPage(
                request: .after(context: context, limit: pageSize, cursor: cursor)
            )
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
            guard frozenAsOfUnixSeconds == nil || frozenAsOfUnixSeconds == page.asOfUnixSeconds else {
                state = .failed(message: "Today changed while loading. Refresh to continue.")
                return
            }
            frozenAsOfUnixSeconds = page.asOfUnixSeconds
            nextCursor = page.nextCursor
            cards = Self.unique(cards + page.items)
            switch state {
            case .offline, .failed:
                break
            default:
                state = cards.isEmpty ? .empty : .loaded
            }
        } catch {
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
            state = Self.failureState(error)
        }
    }

    private static func unique(_ contexts: [TeraLocalNetwork]) -> [TeraLocalNetwork] {
        var identifiers = Set<String>()
        return contexts.filter { identifiers.insert($0.id).inserted }
    }

    private func startObservation() {
        observation.start(
          client: runtimeClient, capacity: 16, delay: observationDelay,
          state: { [weak self] in self?.observationState = $0 },
          change: { [weak self] change in
                switch change.kind {
                case .today, .drafts, .media, .identity, .profile:
                    await self?.reload(refreshProjection: false)
                case .initial, .settings, .relay, .lifecycle:
                    break
                }
            }
        )
    }

    private static func unique(_ cards: [TeraTodayCard]) -> [TeraTodayCard] {
        var identifiers = Set<String>()
        return cards.filter { identifiers.insert($0.id).inserted }
    }

    static func failureState(_ error: Error) -> TeraTodayLoadState {
        let message = TeraUserMessages.text(for: error, fallback: .todayUnavailable)
        if TeraRuntimeFailure.from(error)?.recovery.disposition == .networkUnavailable {
            return .offline(message: message)
        }
        return .failed(message: message)
    }
}
