import Foundation

/// Serial presentation ownership. Paging waits for an active reconciliation;
/// visibility updates can still supersede a page read already in flight.
@MainActor
final class TeraTodayReconciliationTask {
  private var task: Task<Void, Never>?
  private var identity: UUID?

  deinit { task?.cancel() }

  func cancel() {
    task?.cancel()
    task = nil
    identity = nil
  }

  func wait() async {
    while let current = task, !Task.isCancelled {
      // A page waiter does not own this mandatory visibility refresh.
      await current.value
    }
  }

  func run(_ operation: @escaping @MainActor () async -> Void) async {
    cancel()
    let id = UUID()
    identity = id
    let current = Task { [weak self] in
      defer {
        if self?.identity == id {
          self?.task = nil
          self?.identity = nil
        }
      }
      guard !Task.isCancelled else { return }
      await operation()
    }
    task = current
    await withTaskCancellationHandler { await current.value } onCancel: { current.cancel() }
  }
}
