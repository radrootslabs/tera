import Foundation

/// One bounded page and one reader; continuation replaces, never accumulates.
@MainActor
final class TeraSubmissionInventory: ObservableObject {
  @Published private(set) var entries: [TeraSubmissionEntry] = []
  @Published private(set) var cursor: String?
  @Published private(set) var message: String?
  @Published private(set) var isLoading = false
  private let client: TeraRuntimeClient
  private var scope: TeraComposerScope?
  private var generation = TeraSessionGeneration.initial
  private var worker: Task<Void, Never>?
  private var pending: String??

  init(client: TeraRuntimeClient) {
    self.client = client
  }

  deinit { worker?.cancel() }

  func configure(scope: TeraComposerScope) {
    guard self.scope != scope else { return }
    stop()
    self.scope = scope
    entries = []
    cursor = nil
    message = nil
  }

  func stop() {
    generation = generation.invalidated()
    pending = nil
    worker?.cancel()
  }

  func start() {
    pending = .some(nil); schedule()
  }

  func more() {
    guard !isLoading, let cursor else { return }
    pending = .some(cursor)
    schedule()
  }

  private func schedule() {
    guard worker == nil, let next = pending, let scope, generation.isActive else { return }
    pending = nil
    isLoading = true
    let requested = generation
    worker = Task { [weak self] in
      guard let self else { return }
      do {
        let page = try await client.listSubmissions(scope: scope, limit: 100, cursor: next)
        guard generation == requested, !Task.isCancelled else { return finish() }
        guard page.scope == scope, page.entries.count <= 100 else { throw TeraComposerAcknowledgment.unconfirmed }
        entries = page.entries
        cursor = page.nextCursor
        message = nil
      } catch {
        if generation == requested, !Task.isCancelled {
          message = "Saved submissions could not be listed. Keep editing and retry the list."
        }
      }
      finish()
    }
  }

  private func finish() {
    worker = nil
    isLoading = false
    schedule()
  }
}
