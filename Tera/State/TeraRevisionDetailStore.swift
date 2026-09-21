import Foundation

/// One selected revision. Rust owns actions; this owner only fences callbacks.
@MainActor
final class TeraRevisionDetailStore: ObservableObject {
  @Published private(set) var status: TeraRevisionStatus?
  @Published private(set) var message: String?
  @Published private(set) var isWorking = false
  private let operations: TeraRevisionDetailOperations
  private var author: String?
  private var selection: String?
  private var generation = TeraSessionGeneration.initial

  init(client: TeraRuntimeClient) {
    operations = TeraRevisionDetailOperations(
      load: { try await client.revisionStatus(operationID: $0) },
      resume: { try await client.advanceRevision(operationID: $0) },
      cancel: { try await client.cancelRevision(operationID: $0) }
    )
  }

  init(operations: TeraRevisionDetailOperations) {
    self.operations = operations
  }

  func configure(author: String) {
    guard self.author != author else { return }
    stop()
    self.author = author
  }

  func stop() {
    generation = generation.invalidated()
    selection = nil
    status = nil
    message = nil
    isWorking = false
  }

  func load(_ id: String) async {
    generation = generation.invalidated()
    selection = id
    status = nil
    isWorking = false
    await perform(.refresh)
  }

  func refresh() async {
    await perform(.refresh)
  }

  func resume() async {
    await perform(.resume)
  }

  func cancel() async {
    await perform(.cancel)
  }

  private enum Action { case refresh, resume, cancel }

  private func perform(_ action: Action) async {
    guard let id = selection, let author, generation.isActive, !isWorking, !Task.isCancelled else { return }
    let requested = generation
    isWorking = true
    message = nil
    defer {
      if requested == generation {
        isWorking = false
      }
    }
    do {
      var current = try await operations.load(id)
      try validate(current, id: id, author: author, generation: requested)
      switch action {
      case .refresh: break
      case .resume:
        if current.canResume {
          current = try await operations.resume(id)
        }
      case .cancel:
        if current.canCancel {
          current = try await operations.cancel(id)
        }
      }
      try validate(current, id: id, author: author, generation: requested)
      status = current
    } catch {
      guard requested == generation, selection == id, self.author == author, !Task.isCancelled else { return }
      status = nil
      message = "Current revision details are unavailable. Saved work is preserved. Refresh to try again."
    }
  }

  private func validate(_ value: TeraRevisionStatus, id: String, author: String,
                        generation requested: TeraSessionGeneration) throws
  {
    guard requested == generation, generation.isActive, selection == id, self.author == author, !Task.isCancelled,
          value.operationID == id, value.replacement.id == id, value.replacement.isRevision,
          value.replacement.authorPublicKey == author, value.original?.authorPublicKey == author,
          value.retraction.map({ $0.authorPublicKey == author && $0.revisionParentID.map { $0 == id } != false }) != false
    else { throw CancellationError() }
  }
}

struct TeraRevisionDetailOperations: Sendable {
  let load: @Sendable (String) async throws -> TeraRevisionStatus
  let resume: @Sendable (String) async throws -> TeraRevisionStatus
  let cancel: @Sendable (String) async throws -> TeraRevisionStatus
}
