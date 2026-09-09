enum TeraRuntimeResourceCreation {
  static func startup(
    configuration: TeraRuntimeLaunchConfiguration,
    factory: @escaping TeraRuntimeBackendFactory,
    identity: TeraRuntimeOperationIdentity,
    deadlines: TeraRuntimeDeadlinePolicy
  ) -> TeraRuntimeResourceTask<TeraRuntimeBackendStart> {
    TeraRuntimeResourceTask(
      deadlineNanoseconds: deadlines.startupNanoseconds,
      cleanupDeadlineNanoseconds: deadlines.shutdownNanoseconds,
      operation: {
        do {
          return try await .success(factory(configuration))
        } catch {
          return .failure(TeraRuntimeClient.failure(from: error, operation: identity.rawValue))
        }
      },
      cleanup: { started in
        _ = try await started.backend.shutdown()
      }
    )
  }

  static func subscription(
    backend: any TeraRuntimeBackend,
    bufferCapacity: Int,
    identity: TeraRuntimeOperationIdentity,
    deadline: UInt64,
    receive: @escaping @Sendable (TeraRuntimeChange) async -> Void
  ) -> TeraRuntimeResourceTask<any TeraRuntimeSubscriptionToken> {
    TeraRuntimeResourceTask(
      deadlineNanoseconds: deadline,
      cleanupDeadlineNanoseconds: deadline,
      operation: {
        do {
          return try await .success(backend.subscribe(bufferCapacity: bufferCapacity, receive: receive))
        } catch {
          return .failure(TeraRuntimeClient.failure(from: error, operation: identity.rawValue))
        }
      },
      cleanup: { token in await token.cancel() }
    )
  }
}
