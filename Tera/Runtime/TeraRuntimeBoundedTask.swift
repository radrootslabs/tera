import Foundation

enum TeraRuntimeBoundedOutcome<Value: Sendable>: Sendable {
  case completed(Result<Value, TeraRuntimeFailure>)
  case timedOut
  case cancelled
}

/// The wrapper has one immutable state owner; every mutable field is protected
/// by its lock. Task cancellation and continuation resumption occur outside it.
final class TeraRuntimeBoundedTask<Value: Sendable>: @unchecked Sendable {
  typealias Outcome = TeraRuntimeBoundedOutcome<Value>

  final class State: @unchecked Sendable {
    private let lock = NSLock()
    private var outcome: Outcome?
    private var continuations: [UUID: CheckedContinuation<Outcome, Never>] = [:]
    private var cancelledWaiters: Set<UUID> = []
    private var operationTask: Task<Void, Never>?
    private var timeoutTask: Task<Void, Never>?

    func install(operationTask: Task<Void, Never>, timeoutTask: Task<Void, Never>) {
      let terminal = lock.withLock { () -> Outcome? in
        if let outcome {
          return outcome
        }
        self.operationTask = operationTask
        self.timeoutTask = timeoutTask
        return nil
      }
      guard let terminal else { return }
      timeoutTask.cancel()
      switch terminal {
      case .cancelled, .timedOut:
        operationTask.cancel()
      case .completed:
        break
      }
    }

    func value(cancelsOperationWhenWaiterCancelled: Bool) async -> Outcome {
      let waiterID = UUID()
      return await withTaskCancellationHandler {
        await withCheckedContinuation { requestedContinuation in
          let immediate = lock.withLock { () -> Outcome? in
            if let outcome {
              return outcome
            }
            if cancelledWaiters.remove(waiterID) != nil {
              return .cancelled
            }
            continuations[waiterID] = requestedContinuation
            return nil
          }
          if let immediate {
            requestedContinuation.resume(returning: immediate)
          }
        }
      } onCancel: {
        if cancelsOperationWhenWaiterCancelled {
          cancel()
        } else {
          cancelWaiter(waiterID)
        }
      }
    }

    func cancel() {
      guard resolve(.cancelled) else { return }
      let tasks = lock.withLock { (operationTask, timeoutTask) }
      tasks.0?.cancel()
      tasks.1?.cancel()
    }

    func expire() {
      guard resolve(.timedOut) else { return }
      let taskToCancel: Task<Void, Never>? = lock.withLock { self.operationTask }
      taskToCancel?.cancel()
    }

    func finishOperation() {
      lock.withLock { operationTask = nil }
    }

    private func cancelWaiter(_ waiterID: UUID) {
      let continuation = lock.withLock { () -> CheckedContinuation<Outcome, Never>? in
        guard outcome == nil else { return nil }
        guard let continuation = continuations.removeValue(forKey: waiterID) else {
          cancelledWaiters.insert(waiterID)
          return nil
        }
        return continuation
      }
      continuation?.resume(returning: .cancelled)
    }

    @discardableResult
    func resolve(_ requestedOutcome: Outcome) -> Bool {
      var pendingContinuations: [CheckedContinuation<Outcome, Never>] = []
      var timeoutToCancel: Task<Void, Never>?
      let accepted = lock.withLock { () -> Bool in
        guard outcome == nil else { return false }
        outcome = requestedOutcome
        pendingContinuations = Array(continuations.values)
        continuations.removeAll(keepingCapacity: false)
        cancelledWaiters.removeAll(keepingCapacity: false)
        timeoutToCancel = timeoutTask
        return true
      }
      if accepted {
        timeoutToCancel?.cancel()
        for continuation in pendingContinuations {
          continuation.resume(returning: requestedOutcome)
        }
      }
      return accepted
    }
  }

  private let state: State

  init(
    deadlineNanoseconds: UInt64,
    operation: @escaping @Sendable () async -> Result<Value, TeraRuntimeFailure>,
    onAbandonedResult:
    @escaping @Sendable (
      Result<Value, TeraRuntimeFailure>
    ) async -> Void = { _ in }
  ) {
    let state = State()
    self.state = state
    let operationTask = Task { [state] in
      let result = await operation()
      if !state.resolve(.completed(result)) {
        await onAbandonedResult(result)
      }
      state.finishOperation()
    }

    let timeoutTask = Task { [state] in
      do {
        try await Task.sleep(nanoseconds: deadlineNanoseconds)
      } catch {
        return
      }
      state.expire()
    }
    state.install(operationTask: operationTask, timeoutTask: timeoutTask)
  }

  func value(cancelsOperationWhenWaiterCancelled: Bool = true) async -> Outcome {
    await state.value(
      cancelsOperationWhenWaiterCancelled: cancelsOperationWhenWaiterCancelled
    )
  }

  func cancel() {
    state.cancel()
  }
}
