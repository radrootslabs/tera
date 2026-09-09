import Foundation

struct TeraRuntimeShutdownWork: Sendable {
  let backend: (any TeraRuntimeBackend)?
  let drains: [@Sendable () async throws -> Void]

  func close() async -> Result<TeraRuntimeShutdownReceipt, TeraRuntimeFailure> {
    do {
      for drain in drains {
        try await drain()
      }
      guard let backend else { return .success(.alreadyStopped) }
      return try await .success(backend.shutdown())
    } catch {
      return .failure(TeraRuntimeClient.failure(from: error, operation: "runtime.shutdown"))
    }
  }
}

/// One retained shutdown operation; each caller has a bounded wait. Cancelling
/// or timing out a wait never cancels this owner or starts another close.
final class TeraRuntimeShutdownTask: Sendable {
  private let task: Task<Result<TeraRuntimeShutdownReceipt, TeraRuntimeFailure>, Never>
  private let deadline: UInt64

  init(
    work: TeraRuntimeShutdownWork,
    deadline: UInt64,
    completed: @escaping @Sendable (Result<TeraRuntimeShutdownReceipt, TeraRuntimeFailure>) async -> Void
  ) {
    self.deadline = deadline
    task = Task {
      let result = await work.close()
      await completed(result)
      return result
    }
  }

  func value() async -> TeraRuntimeBoundedOutcome<TeraRuntimeShutdownReceipt> {
    let wait = TeraRuntimeBoundedTask(deadlineNanoseconds: deadline) { [task] in
      await task.value
    }
    return await wait.value()
  }
}
