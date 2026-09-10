import Foundation

extension TeraRuntimeClient {
  func reserveComposerID() async throws -> String {
    try await addOperation("runtime.composer.reserve") { backend in
      try await backend.reserveComposerID()
    }
  }

  func saveComposer(request: TeraComposerSaveRequest) async throws -> TeraComposerSaveReceipt {
    try await addOperation("runtime.composer.save") { backend in
      try await backend.saveComposer(request: request)
    }
  }

  func loadComposer(scope: TeraComposerScope, id: String) async throws -> TeraComposerDraft {
    try await addOperation("runtime.composer.load") { backend in
      try await backend.loadComposer(scope: scope, id: id)
    }
  }

  func listComposers(scope: TeraComposerScope, limit: UInt16 = 100, cursor: String? = nil) async throws -> TeraComposerPage {
    try await addOperation("runtime.composer.list") { backend in
      try await backend.listComposers(scope: scope, limit: limit, cursor: cursor)
    }
  }

  func addOperation<T: Sendable>(
    _ operation: String,
    _ body: @escaping @Sendable (any TeraRuntimeBackend) async throws -> T
  ) async throws -> T {
    do {
      return try await runtimeOperation(operation, body)
    } catch let error as TeraRuntimeClientError {
      throw error
    } catch {
      throw TeraRuntimeClientError.add(
        Self.failure(from: error, operation: operation)
      )
    }
  }
}
