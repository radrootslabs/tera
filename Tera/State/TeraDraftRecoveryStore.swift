import Foundation

/// One reader and one pending selection request. Only the current bounded
/// composer and legacy pages are retained; continuation never grows an archive.
@MainActor
final class TeraDraftRecoveryStore: ObservableObject {
  static let pageSize: UInt16 = 100
  @Published private(set) var composers: [TeraComposerListEntry] = []
  @Published private(set) var legacy: [TeraLegacyDraftListEntry] = []
  @Published private(set) var composerCursor: String?
  @Published private(set) var legacyCursor: String?
  @Published private(set) var composerError: String?
  @Published private(set) var legacyError: String?
  @Published private(set) var isLoading = false
  private(set) var scope: TeraComposerScope?
  private let client: TeraRuntimeClient
  private var generation = TeraSessionGeneration.initial
  private var task: Task<Void, Never>?
  private var pending: Request?

  private enum Request { case first, composers(String), legacy(String) }

  init(client: TeraRuntimeClient) {
    self.client = client
  }

  deinit { task?.cancel() }

  func configure(scope: TeraComposerScope) {
    guard self.scope != scope else { return }
    stop()
    self.scope = scope
    composers = []
    legacy = []
    composerCursor = nil
    legacyCursor = nil
    composerError = nil
    legacyError = nil
  }

  func stop() {
    generation = generation.invalidated()
    pending = nil
    task?.cancel()
    isLoading = false
  }

  func start() {
    schedule(.first)
  }

  func load(_ selection: TeraDraftRecoverySelection) async throws -> TeraRecoveredDraft {
    guard let scope else { throw TeraComposerAcknowledgment.unconfirmed }
    let requested = generation
    let result: TeraRecoveredDraft
    switch selection {
    case let .composer(id):
      let draft = try await client.loadComposer(scope: scope, id: id)
      guard draft.scope == scope, draft.id == id else { throw TeraComposerAcknowledgment.unconfirmed }
      result = .composer(draft)
    case let .legacy(id):
      let draft = try await client.draftStatus(id: id)
      guard draft.authorPublicKey == scope.authorPublicKey, draft.id == id, draft.form != nil else {
        throw TeraComposerAcknowledgment.unconfirmed
      }
      result = .legacy(draft)
    }
    guard isCurrent(requested) else { throw CancellationError() }
    return result
  }

  func moreComposers() {
    guard let composerCursor, !isLoading else { return }
    schedule(.composers(composerCursor))
  }

  func moreLegacy() {
    guard let legacyCursor, !isLoading else { return }
    schedule(.legacy(legacyCursor))
  }

  private func schedule(_ request: Request) {
    guard !Task.isCancelled, generation.isActive, scope != nil else { return }
    pending = request
    isLoading = true
    startWorker()
  }

  private func startWorker() {
    guard task == nil, let request = pending, let scope, generation.isActive else { return }
    pending = nil
    let requested = generation
    isLoading = true
    task = Task { [weak self] in
      guard let self else { return }
      await load(request, scope: scope, generation: requested)
      task = nil
      isLoading = false
      startWorker()
    }
  }

  private func load(_ request: Request, scope: TeraComposerScope, generation: TeraSessionGeneration) async {
    switch request {
    case .first:
      await loadComposers(scope: scope, cursor: nil, generation: generation)
      guard isCurrent(generation) else { return }
      await loadLegacy(scope: scope, cursor: nil, generation: generation)
    case let .composers(cursor):
      await loadComposers(scope: scope, cursor: cursor, generation: generation)
    case let .legacy(cursor):
      await loadLegacy(scope: scope, cursor: cursor, generation: generation)
    }
  }

  private func loadComposers(scope: TeraComposerScope, cursor: String?, generation: TeraSessionGeneration) async {
    do {
      let page = try await client.listComposers(scope: scope, limit: Self.pageSize, cursor: cursor)
      guard isCurrent(generation) else { return }
      guard page.scope == scope, page.entries.count <= Int(Self.pageSize) else {
        throw TeraComposerAcknowledgment.unconfirmed
      }
      composers = page.entries
      composerCursor = page.nextCursor
      composerError = nil
    } catch {
      guard isCurrent(generation) else { return }
      composerError = "Saved editing could not be listed. Keep current changes and retry the list."
    }
  }

  private func loadLegacy(scope: TeraComposerScope, cursor: String?, generation: TeraSessionGeneration) async {
    do {
      let page = try await client.legacyDraftPage(limit: Self.pageSize, cursor: cursor)
      guard isCurrent(generation) else { return }
      guard page.authorPublicKey == scope.authorPublicKey, page.entries.count <= Int(Self.pageSize) else {
        throw TeraComposerAcknowledgment.unconfirmed
      }
      legacy = page.entries
      legacyCursor = page.nextCursor
      legacyError = nil
    } catch {
      guard isCurrent(generation) else { return }
      legacyError = "Saved operations could not be listed. Editing is still available; retry the list."
    }
  }

  private func isCurrent(_ requested: TeraSessionGeneration) -> Bool {
    generation == requested && generation.isActive && !Task.isCancelled
  }
}
