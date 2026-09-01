import Foundation

enum RadrootsTodayLoadState: Sendable, Equatable {
    case idle
    case loading
    case loaded
    case empty
    case offline(message: String)
    case failed(message: String)
}

@MainActor
final class RadrootsTodayStore: ObservableObject {
    @Published private(set) var contexts: [RadrootsLocalNetwork]
    @Published private(set) var selectedContextID: String?
    @Published private(set) var cards: [RadrootsTodayCard] = []
    @Published private(set) var state: RadrootsTodayLoadState = .idle
    @Published private(set) var isLoadingNextPage = false
    @Published private(set) var observationState: RadrootsRuntimeObservationState = .inactive

    private let runtimeClient: RadrootsRuntimeClient
    private let pageSize: UInt16
    private let clock: RadrootsClock
    private let observationDelay: @Sendable (UInt32) async throws -> Void
    private var frozenAsOfUnixSeconds: UInt64?
    private var nextCursor: String?
    private var requestGeneration: UInt64 = 0
    private var observationTask: Task<Void, Never>?
    private var reloadTask: Task<Void, Never>?
    private var isStarted = false
    private var observationGeneration: UInt64 = 0

    init(
      runtimeClient: RadrootsRuntimeClient,
      contexts: [RadrootsLocalNetwork] = [],
      selectedContextID: String? = nil,
      pageSize: UInt16 = 20,
      clock: RadrootsClock = .system,
      observationDelay: @escaping @Sendable (UInt32) async throws -> Void =
            RadrootsRuntimeObservationBackoff.sleep
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
        observationTask?.cancel()
        reloadTask?.cancel()
    }

    var selectedContext: RadrootsLocalNetwork? {
        contexts.first(where: { $0.id == selectedContextID })
    }

    var canLoadNextPage: Bool {
        nextCursor != nil && !isLoadingNextPage
    }

    func configure(snapshot: RadrootsRuntimeSnapshot) {
        guard contexts.isEmpty else { return }
        let context = RadrootsLocalNetwork.defaultContext(snapshot: snapshot)
        contexts = [context]
        selectedContextID = context.id
    }

    func start() async {
        guard !isStarted else { return }
        isStarted = true
        observationGeneration &+= 1
        let generation = observationGeneration
        observationTask = Task { [weak self] in
            await self?.observe(generation: generation)
        }
        await reload()
    }

    func stop() {
        isStarted = false
        observationGeneration &+= 1
        requestGeneration &+= 1
        observationTask?.cancel()
        observationTask = nil
        observationState = .stopped
        reloadTask?.cancel()
        reloadTask = nil
        isLoadingNextPage = false
    }

    func selectContext(id: String) {
        guard id != selectedContextID,
              contexts.contains(where: { $0.id == id })
        else {
            return
        }
        selectedContextID = id
        requestGeneration &+= 1
        reloadTask?.cancel()
        reloadTask = Task { [weak self] in
            await self?.reload()
        }
    }

    func replaceContexts(_ updatedContexts: [RadrootsLocalNetwork], selectedID: String?) {
        let updatedContexts = Self.unique(updatedContexts)
        guard !updatedContexts.isEmpty else { return }
        contexts = updatedContexts
        selectedContextID =
            selectedID.flatMap { requested in
                updatedContexts.contains(where: { $0.id == requested }) ? requested : nil
            } ?? updatedContexts.first?.id
        requestGeneration &+= 1
        reloadTask?.cancel()
        reloadTask = Task { [weak self] in
            await self?.reload()
        }
    }

    func reload(
      refreshProjection: Bool = true,
      update: RadrootsTodayProjectionUpdate = .incremental
    ) async {
        guard let context = selectedContext else {
            cards = []
            state = .failed(message: "Choose a local network to load Today.")
            return
        }

        requestGeneration &+= 1
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

        do {
            let asOf = try clock.unixSeconds()
            let page = try await runtimeClient.todayPage(
                request: .first(
                  context: context,
                  limit: pageSize,
                  asOfUnixSeconds: asOf
                )
            )
            guard generation == requestGeneration, !Task.isCancelled else { return }
            frozenAsOfUnixSeconds = page.asOfUnixSeconds
            nextCursor = page.nextCursor
            cards = Self.unique(page.items)
            state = refreshFailure.map(Self.failureState) ?? (cards.isEmpty ? .empty : .loaded)
        } catch {
            guard generation == requestGeneration, !Task.isCancelled else { return }
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
            guard generation == requestGeneration, !Task.isCancelled else { return }
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
            guard generation == requestGeneration, !Task.isCancelled else { return }
            state = Self.failureState(error)
        }
    }

    private static func unique(_ contexts: [RadrootsLocalNetwork]) -> [RadrootsLocalNetwork] {
        var identifiers = Set<String>()
        return contexts.filter { identifiers.insert($0.id).inserted }
    }

    private func observe(generation: UInt64) async {
        var attempt: UInt32 = 0
        while isStarted, observationGeneration == generation, !Task.isCancelled {
            observationState = .subscribing(attempt: attempt &+ 1)
            var failureMessage = RadrootsUserMessages.text(.runtimeObservationUnavailable)
            do {
                let changes = try await runtimeClient.changes(bufferCapacity: 16)
                observationState = .active
                for await change in changes {
                    guard !Task.isCancelled else { break }
                    attempt = 0
                    switch change.kind {
                    case .today, .drafts, .media, .identity, .profile:
                        await reload(refreshProjection: false)
                    case .initial, .settings, .relay, .lifecycle:
                        continue
                    }
                }
            } catch {
                failureMessage = RadrootsUserMessages.text(
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

    private static func unique(_ cards: [RadrootsTodayCard]) -> [RadrootsTodayCard] {
        var identifiers = Set<String>()
        return cards.filter { identifiers.insert($0.id).inserted }
    }

    private static func failureState(_ error: Error) -> RadrootsTodayLoadState {
        let failure: RadrootsRuntimeFailure? =
            if case let RadrootsRuntimeClientError.today(value) = error {
                value
            } else if case let RadrootsRuntimeClientError.status(value) = error {
                value
            } else {
                error as? RadrootsRuntimeFailure
            }
        let message = RadrootsUserMessages.text(for: error, fallback: .todayUnavailable)
        guard let failure else { return .failed(message: message) }
        let category = failure.category.lowercased()
        if failure.retryable || category.contains("network") || category.contains("relay") {
            return .offline(message: message)
        }
        return .failed(message: message)
    }
}
