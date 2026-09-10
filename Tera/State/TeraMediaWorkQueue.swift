import Foundation

/// Cancelled work retains its physical slot until its operation returns.
@MainActor
final class TeraMediaWorkQueue {
  private struct Pending {
    let id: UUID
    let operation: @MainActor () async -> Void
  }

  private let limits: TeraMediaPresentationLimits
  private var active: [UUID: Task<Void, Never>] = [:]
  private var pending: [Pending] = []

  init(limits: TeraMediaPresentationLimits) {
    self.limits = limits
  }

  deinit { for task in active.values {
    task.cancel()
  } }

  var activeCount: Int {
    active.count
  }

  var queuedCount: Int {
    pending.count
  }

  func submit(id: UUID, operation: @escaping @MainActor () async -> Void) -> Bool {
    guard active[id] == nil, !pending.contains(where: { $0.id == id }),
      active.count < limits.workers || pending.count < limits.queuedRequests else { return false }
    pending.append(Pending(id: id, operation: operation))
    drain()
    return true
  }

  func cancel(id: UUID) {
    pending.removeAll { $0.id == id }
    active[id]?.cancel()
  }

  func cancelAll() {
    pending.removeAll()
    for task in active.values {
      task.cancel()
    }
  }

  private func drain() {
    while active.count < limits.workers, !pending.isEmpty {
      let work = pending.removeFirst()
      active[work.id] = Task { [weak self] in
        await work.operation()
        self?.active[work.id] = nil
        self?.drain()
      }
    }
  }
}
