import Foundation

/// Creation completion and adoption are separate events. Only the transition
/// to abandoned may start cleanup; adoption permanently transfers that duty.
final class TeraRuntimeResourceTask<Value: Sendable>: Sendable {
  private enum State {
    case pending
    case created(Value)
    case adopted
    case abandoned
  }

  private let ownership: Ownership
  private let task: TeraRuntimeBoundedTask<Value>

  init(
    deadlineNanoseconds: UInt64,
    cleanupDeadlineNanoseconds: UInt64,
    operation: @escaping @Sendable () async -> Result<Value, TeraRuntimeFailure>,
    cleanup: @escaping @Sendable (Value) async throws -> Void
  ) {
    let ownership = Ownership(deadline: cleanupDeadlineNanoseconds, cleanup: cleanup)
    self.ownership = ownership
    task = TeraRuntimeBoundedTask(deadlineNanoseconds: deadlineNanoseconds) {
      let result = await operation()
      if case let .success(value) = result {
        ownership.created(value)
      }
      return result
    } onAbandonedResult: { _ in
      ownership.abandon()
    }
  }

  func value(cancelsOperationWhenWaiterCancelled: Bool = true) async -> TeraRuntimeBoundedOutcome<Value> {
    let outcome = await task.value(cancelsOperationWhenWaiterCancelled: cancelsOperationWhenWaiterCancelled)
    switch outcome {
    case .timedOut:
      ownership.abandon()
    case .cancelled where cancelsOperationWhenWaiterCancelled:
      ownership.abandon()
    default:
      break
    }
    return outcome
  }

  func adopt() -> Bool {
    ownership.adopt()
  }

  func cancel() {
    ownership.abandon()
    task.cancel()
  }

  /// Drain creation and its cleanup, retaining a failed resource for a later
  /// explicit close attempt. This wait belongs to the bounded shutdown owner.
  func finishAbandonment() async throws {
    cancel()
    if case .failure = await task.settle() {
      return
    }
    try await ownership.finishCleanup()
  }

  /// Every mutable field is lock protected. Cleanup starts outside the lock,
  /// has the existing host deadline, and retains its resource until it returns.
  private final class Ownership: @unchecked Sendable {
    private let lock = NSLock()
    private var state = State.pending
    private var cleanupTask: Cleanup?
    private var cleanupWaiters: [CheckedContinuation<Cleanup?, Never>] = []
    private let deadline: UInt64
    private let cleanup: @Sendable (Value) async throws -> Void

    init(deadline: UInt64, cleanup: @escaping @Sendable (Value) async throws -> Void) {
      self.deadline = deadline
      self.cleanup = cleanup
    }

    func created(_ value: Value) {
      let abandoned = lock.withLock {
        if case .abandoned = state {
          return true
        }
        state = .created(value)
        return false
      }
      if abandoned {
        startCleanup(value)
      }
    }

    func adopt() -> Bool {
      lock.withLock {
        guard case .created = state else { return false }
        state = .adopted
        return true
      }
    }

    func abandon() {
      let value: Value? = lock.withLock {
        switch state {
        case let .created(value):
          state = .abandoned
          return value
        case .pending:
          state = .abandoned
          return nil
        case .adopted, .abandoned:
          return nil
        }
      }
      if let value {
        startCleanup(value)
      }
    }

    private func startCleanup(_ value: Value) {
      let task = Cleanup(value: value, deadline: deadline, cleanup: cleanup)
      let waiters = lock.withLock {
        cleanupTask = task
        let pending = cleanupWaiters
        cleanupWaiters.removeAll()
        return pending
      }
      for waiter in waiters {
        waiter.resume(returning: task)
      }
    }

    func finishCleanup() async throws {
      let task = await withCheckedContinuation { continuation in
        let immediate = lock.withLock { () -> (Bool, Cleanup?) in
          if case .adopted = state {
            return (true, nil)
          }
          if let cleanupTask {
            return (true, cleanupTask)
          }
          cleanupWaiters.append(continuation)
          return (false, nil)
        }
        if immediate.0 {
          continuation.resume(returning: immediate.1)
        }
      }
      try await task?.finish()
    }
  }

  private actor Cleanup {
    private let operation: @Sendable () async -> Result<Void, TeraRuntimeFailure>
    private let deadline: UInt64
    private var task: TeraRuntimeBoundedTask<Void>

    init(value: Value, deadline: UInt64, cleanup: @escaping @Sendable (Value) async throws -> Void) {
      let operation: @Sendable () async -> Result<Void, TeraRuntimeFailure> = {
        do {
          try await cleanup(value)
          return .success(())
        } catch {
          return .failure(TeraRuntimeClient.failure(from: error, operation: "runtime.resource.close"))
        }
      }
      self.operation = operation
      self.deadline = deadline
      task = TeraRuntimeBoundedTask(deadlineNanoseconds: deadline, operation: operation)
    }

    func finish() async throws {
      if case .failure = task.settlement() {
        task = TeraRuntimeBoundedTask(deadlineNanoseconds: deadline, operation: operation)
      }
      try await task.settle().get()
    }
  }
}
