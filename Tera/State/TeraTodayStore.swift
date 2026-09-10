import Foundation

@MainActor
final class TeraTodayStore: ObservableObject {
    @Published private(set) var contexts: [TeraLocalNetwork]
    @Published private(set) var selectedContextID: String?
    @Published private(set) var cards: [TeraTodayCard] = []
    @Published private(set) var presentation = TeraTodayPresentation()
    @Published private(set) var discovery = TeraTodayDiscoveryPresentation()
    private var discoveryGeneration = TeraSessionGeneration.initial
    @Published private(set) var isLoadingNextPage = false
    @Published private(set) var observationState: TeraRuntimeObservationState = .inactive
    @Published private(set) var scopeGeneration = TeraSessionGeneration.initial
    @Published private(set) var hasPendingContent = false
    var mediaWillChange: ([TeraMediaReference], [TeraMediaReference], TeraLocalNetwork?) -> Void = { _, _, _ in }
    private var projectionGeneration: UInt64?
    var scopeWillChange: (TeraLocalNetwork?) -> Void = { _ in }

    private let runtimeClient: TeraRuntimeClient
    private let pageSize: UInt16
    private let clock: TeraClock
    private let observationDelay: @Sendable (UInt32) async throws -> Void
    private var frozenAsOfUnixSeconds: UInt64?
    private var nextCursor: String?
    private var requestGeneration = TeraSessionGeneration.initial
    private let observation = TeraStoreObservation()
    private let reconciliation = TeraTodayReconciliationTask()
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
        self.contexts = TeraTodayReconciler.unique(contexts)
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

    func configure(snapshot: TeraRuntimeSnapshot) {
        let updated = TeraPresentationConfiguration(snapshot: snapshot)
        guard configuration != updated else { return }
        let reload = observation.isActive
        invalidatePresentation(for: updated.context)
        configuration = updated
        // The runtime snapshot currently supplies one default local network.
        // Reconcile even the first configuration; injected choices are not an
        // authority for a different account or runtime profile.
        contexts = [updated.context]
        selectedContextID = updated.context.id
        if reload {
          scheduleReload()
        }
    }

    func start() async {
        guard !observation.isActive, !Task.isCancelled else { return }
        startObservation()
        scheduleReload()
        let task = reloadTask
        await withTaskCancellationHandler { await task?.value } onCancel: { task?.cancel() }
    }

    func stop() {
        observation.stop()
        reconciliation.cancel()
        observationState = .stopped
        discoveryGeneration = discoveryGeneration.invalidated()
        discovery.stop()
        requestGeneration = requestGeneration.invalidated()
        reloadTask?.cancel()
        reloadTask = nil
        isLoadingNextPage = false
        presentation.stop()
    }

    private func invalidatePresentation(for context: TeraLocalNetwork?) {
        resetDiscovery()
        reconciliation.cancel()
        hasPendingContent = false
        projectionGeneration = nil
        requestGeneration = requestGeneration.invalidated()
        reloadTask?.cancel()
        reloadTask = nil
        cards = []
        frozenAsOfUnixSeconds = nil
        nextCursor = nil
        isLoadingNextPage = false
        presentation = TeraTodayPresentation()
        scopeWillChange(context)
        scopeGeneration = scopeGeneration.invalidated()
        if observation.isActive {
            observation.stop()
            startObservation()
        }
    }

    func selectContext(id: String) {
        guard id != selectedContextID,
              let context = contexts.first(where: { $0.id == id })
        else {
            return
        }
        invalidatePresentation(for: context)
        selectedContextID = id
        scheduleReload()
    }

    func replaceContexts(_ updatedContexts: [TeraLocalNetwork], selectedID: String?) {
        let updatedContexts = TeraTodayReconciler.unique(updatedContexts)
        let selected = [selectedID, selectedContextID].compactMap(\.self).first { requested in
            updatedContexts.contains(where: { $0.id == requested })
        } ?? updatedContexts.first?.id
        guard contexts != updatedContexts || selectedContextID != selected else { return }
        invalidatePresentation(for: updatedContexts.first(where: { $0.id == selected }))
        contexts = updatedContexts
        selectedContextID = selected
        scheduleReload()
    }

    private func scheduleReload() {
        reloadTask = Task { [weak self] in await self?.reload() }
    }

    func reload(
      refreshProjection: Bool = true,
      update: TeraTodayProjectionUpdate = .incremental
    ) async {
        guard !Task.isCancelled else { return }
        guard let context = selectedContext else {
            cards = []
            presentation = TeraTodayPresentation()
            presentation.failRead(.failed(message: "Choose a local network to load Today."))
            return
        }

        if refreshProjection {
          resetDiscovery()
        }
        requestGeneration = requestGeneration.invalidated()
        var generation = requestGeneration
        defer {
            if generation == requestGeneration {
              presentation.stop()
            }
        }
        frozenAsOfUnixSeconds = nil
        nextCursor = nil
        isLoadingNextPage = false
        hasPendingContent = false
        presentation.beginReload()
        guard generation.isActive else { return }
        await readFirstPage(context: context, generation: generation, receipt: nil)
        guard refreshProjection, generation == requestGeneration, !Task.isCancelled else { return }
        let receipt = await refresh(context: context, update: update, generation: generation)
        guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
        // Invalidate pagination from the cached page before reading the updated
        // projection, even when the new page has the same as-of timestamp.
        requestGeneration = requestGeneration.invalidated()
        generation = requestGeneration
        frozenAsOfUnixSeconds = nil
        nextCursor = nil
        isLoadingNextPage = false
        guard generation.isActive else { return }
        await readFirstPage(context: context, generation: generation, receipt: receipt)
    }

    private func refresh(
      context: TeraLocalNetwork, update: TeraTodayProjectionUpdate, generation: TeraSessionGeneration
    ) async -> TeraTodayRefreshReceipt? {
        presentation.beginRefresh()
        do {
            let receipt = try await runtimeClient.refreshToday(
              context: context, nowUnixSeconds: clock.unixSeconds(), update: update
            )
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return nil }
            presentation.refreshCompleted(receipt)
            discovery.accept(receipt.discovery)
            return receipt.projection
        } catch {
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return nil }
            presentation.refreshFailed(error)
            return nil
        }
    }

    private func readFirstPage(
      context: TeraLocalNetwork, generation: TeraSessionGeneration, receipt: TeraTodayRefreshReceipt?
    ) async {
        presentation.beginRead()
        defer {
            if generation == requestGeneration {
              presentation.finishReading()
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
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
            frozenAsOfUnixSeconds = page.asOfUnixSeconds
            nextCursor = page.nextCursor
            projectionGeneration = page.projectionGeneration
            replaceCards(TeraTodayReconciler.unique(page.items))
            presentation.acceptPage(count: cards.count, receipt: receipt)
        } catch {
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
            presentation.failRead(TeraTodayFailure(error))
        }
    }

    func loadNextPage() async {
        let requestedScope = scopeGeneration
        await reconciliation.wait()
        guard requestedScope == scopeGeneration, !Task.isCancelled else { return }
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
            guard projectionGeneration == nil || projectionGeneration == page.projectionGeneration,
              frozenAsOfUnixSeconds == nil || frozenAsOfUnixSeconds == page.asOfUnixSeconds
            else {
                failPagination(.staleCursor(message: "Today changed while loading. Refresh to continue."))
                return
            }
            frozenAsOfUnixSeconds = page.asOfUnixSeconds
            nextCursor = page.nextCursor
            replaceCards(TeraTodayReconciler.unique(cards + page.items))
            presentation.acceptPage(count: cards.count)
        } catch {
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
            failPagination(TeraTodayFailure(error))
        }
    }
}

private extension TeraTodayStore {
    private func startObservation() {
        observation.start(
          client: runtimeClient, buffer: (capacity: 16, delay: observationDelay),
          state: { [weak self] in self?.observationState = $0 },
          accepts: { [weak self] in $0.matches(context: self?.selectedContext) },
          refresh: { [weak self] batch in
                // Ordinary media progress belongs to the media presentation;
                // it must not reset the feed's loaded pages and cursor.
                guard batch.contains(anyOf: [.today, .drafts, .identity, .profile]) else { return }
                await self?.reloadTask?.value
                guard !Task.isCancelled else { return }
                await self?.reconciliation.run { [weak self] in await self?.reconcileLoadedCards() }
            }
        )
    }

    func replaceCards(_ updated: [TeraTodayCard]) {
        mediaWillChange(cards.flatMap(\.media), updated.flatMap(\.media), selectedContext)
        cards = updated
    }

    func reconcileLoadedCards() async {
        guard let context = selectedContext, let asOf = frozenAsOfUnixSeconds else { return }
        requestGeneration = requestGeneration.invalidated()
        let generation = requestGeneration
        isLoadingNextPage = false
        presentation.beginRead()
        do {
            let page = try await TeraTodayReconciler.read(
              client: runtimeClient, context: context, asOf: asOf, cards: cards
            )
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
            if projectionGeneration != page.projectionGeneration {
                hasPendingContent = true
                nextCursor = nil
            }
            replaceCards(page.items)
            presentation.acceptPage(count: cards.count)
        } catch {
            guard generation == requestGeneration, generation.isActive, !Task.isCancelled else { return }
            // Visibility could not be established. An old card or photo must
            // not remain authoritative after a failed mandatory resnapshot.
            replaceCards([])
            nextCursor = nil
            hasPendingContent = true
            presentation.acceptPage(count: 0)
            presentation.failRead(TeraTodayFailure(error))
        }
    }

    func resetDiscovery() {
        discoveryGeneration = discoveryGeneration.invalidated()
        discovery = TeraTodayDiscoveryPresentation()
    }

    func failPagination(_ failure: TeraTodayFailure) {
        if failure.requiresRefresh {
          nextCursor = nil
        }
        presentation.failRead(failure)
    }
}

extension TeraTodayStore {
    func currentCard(id: String) -> TeraTodayCard? {
      cards.first { $0.id == id }
    }

    var selectedContext: TeraLocalNetwork? {
        contexts.first(where: { $0.id == selectedContextID })
    }

    var canLoadNextPage: Bool {
        nextCursor != nil && !isLoadingNextPage
    }

    func searchOlderPosts() async {
        guard !Task.isCancelled, let context = selectedContext,
              let cursor = discovery.continuation, discovery.canSearchOlder
        else { return }
        discoveryGeneration = discoveryGeneration.invalidated()
        let generation = discoveryGeneration
        guard generation.isActive else { return }
        discovery.begin()
        defer {
            if generation == discoveryGeneration {
              discovery.stop()
            }
        }
        do {
            let receipt = try await runtimeClient.refreshToday(
              context: context, nowUnixSeconds: clock.unixSeconds(), backfillCursor: cursor
            )
            guard generation == discoveryGeneration, !Task.isCancelled else { return }
            discovery.accept(receipt.discovery)
            // An explicit older search can refresh the local view. Ordinary
            // local reloads cannot erase this search's continuation/evidence.
            await reload(refreshProjection: false)
            guard generation == discoveryGeneration, !Task.isCancelled else { return }
            presentation.refreshCompleted(receipt)
            if presentation.readFailure == nil {
                presentation.acceptPage(count: cards.count, receipt: receipt.projection)
            }
        } catch {
            guard generation == discoveryGeneration, !Task.isCancelled else { return }
            discovery.fail(error)
        }
    }
}
