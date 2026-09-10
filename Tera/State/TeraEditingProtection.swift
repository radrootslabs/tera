import Foundation

/// One replacement and one worker. These transient tokens are never persisted
/// as composer identity, revision, or publication authority.
@MainActor
final class TeraEditingProtection: ObservableObject {
  enum Kind { case editing, reopen }
  @Published private(set) var failed = false
  @Published private(set) var isWorking = false
  @Published private(set) var reopened: UUID?
  var cancelled: () -> Void = {}
  private struct Pending {
    let token: UUID
    let kind: Kind
    let save: () async -> Bool
    let apply: () async -> Bool
  }

  private var pending: Pending?
  private var worker: Task<Bool, Never>?
  private var activeToken: UUID?

  var choiceToken: UUID? {
    pending?.token
  }

  deinit { worker?.cancel() }

  func replace(kind: Kind, save: @escaping () async -> Bool, apply: @escaping () async -> Bool) async -> Bool {
    guard let token = schedule(kind: kind, save: save, apply: apply), let worker else { return false }
    return await withTaskCancellationHandler { await worker.value } onCancel: {
      Task { @MainActor [weak self] in self?.cancel(token: token) }
    }
  }

  @discardableResult
  func schedule(kind: Kind, save: @escaping () async -> Bool, apply: @escaping () async -> Bool) -> UUID? {
    guard pending == nil, worker == nil, !Task.isCancelled else { return nil }
    let token = UUID()
    pending = Pending(token: token, kind: kind, save: save, apply: apply)
    start(save: true)
    return token
  }

  func retry() {
    start(save: true)
  }

  func discard() {
    start(save: false)
  }

  func discard(token: UUID) {
    guard pending?.token == token else { return }
    discard()
  }

  func cancel() {
    let hadRequest = pending != nil || activeToken != nil
    pending = nil
    activeToken = nil
    failed = false
    isWorking = false
    worker?.cancel()
    if hadRequest {
      cancelled()
    }
  }

  func editingChanged() {
    // Once the replacement is admitted, the Add owner's operation generation
    // fences its callbacks. Its own form assignment is not a new dirty edit.
    if pending != nil {
      cancel()
    }
  }

  func cancel(token: UUID) {
    if pending?.token == token || activeToken == token {
      cancel()
    }
  }

  private func start(save: Bool) {
    guard worker == nil, let pending, !Task.isCancelled else { return }
    failed = false
    isWorking = true
    activeToken = pending.token
    worker = Task { [weak self] in
      guard let self else { return false }
      return await run(pending, save: save)
    }
  }

  private func run(_ request: Pending, save: Bool) async -> Bool {
    defer {
      worker = nil
      isWorking = false
      activeToken = nil
    }
    let saved = if save {
      await request.save()
    } else {
      true
    }
    guard !Task.isCancelled, pending?.token == request.token, activeToken == request.token else { return false }
    guard saved else {
      failed = true
      return false
    }
    pending = nil
    let applied = await request.apply()
    guard !Task.isCancelled, activeToken == request.token, applied else { return false }
    if request.kind == .reopen {
      reopened = request.token
    }
    return true
  }
}
