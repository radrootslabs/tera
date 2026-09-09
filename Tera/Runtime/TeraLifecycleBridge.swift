import Foundation

final class TeraCompletionOnce: @unchecked Sendable {
  private let lock = NSLock()
  private var completion: (() -> Void)?

  init(_ completion: @escaping () -> Void) {
    self.completion = completion
  }

  func complete() {
    let action = lock.withLock {
      let action = completion
      completion = nil
      return action
    }
    action?()
  }
}

actor TeraLifecycleBridge {
  static let shared = TeraLifecycleBridge()

  private var registration = UUID()
  private var shutdown: (@Sendable () async -> Bool)?
  private struct Attempt {
    let id: UUID
    let registration: UUID
    let task: Task<Bool, Never>
  }

  private var active: Attempt?

  func register(shutdown: @escaping @Sendable () async -> Bool) {
    registration = UUID()
    self.shutdown = shutdown
  }

  func requestShutdown() async {
    let id = registration
    let attempt: Attempt
    if let active, active.registration == id {
      attempt = active
    } else if let shutdown {
      attempt = Attempt(id: UUID(), registration: id, task: Task { await shutdown() })
      active = attempt
    } else {
      return
    }
    let completed = await attempt.task.value
    if active?.id == attempt.id {
      active = nil
    }
    if registration == id, completed {
      shutdown = nil
    }
    if registration == id {
      await TeraBackgroundEventRouter.shared.detachAndCompletePending()
    }
  }
}
