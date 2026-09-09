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
    cleanup: @escaping @Sendable (Value) async -> Void
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

  /// Every mutable field is lock protected. Cleanup starts outside the lock,
  /// has the existing host deadline, and retains its resource until it returns.
  private final class Ownership: @unchecked Sendable {
    private let lock = NSLock()
    private var state = State.pending
    private var cleanupTask: TeraRuntimeBoundedTask<Void>?
    private let deadline: UInt64
    private let cleanup: @Sendable (Value) async -> Void

    init(deadline: UInt64, cleanup: @escaping @Sendable (Value) async -> Void) {
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
      let task = TeraRuntimeBoundedTask<Void>(deadlineNanoseconds: deadline) { [cleanup] in
        await cleanup(value)
        return .success(())
      }
      lock.withLock { cleanupTask = task }
    }
  }
}
