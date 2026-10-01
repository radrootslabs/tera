import Foundation
import TeraKitBindings

struct TeraKeyRemovalRequest: Sendable, Equatable {
  let id: String
  let revision: UInt64
}

extension TeraRuntimeClient {
  func prepareRetractionForKeyRemoval(_ request: TeraKeyRemovalRequest) async throws {
    try await addOperation("runtime.keyRemoval.prepare") { backend in
      try await backend.prepareRetractionForKeyRemoval(request)
    }
  }
}

extension TeraGeneratedRuntimeBackend {
  func prepareRetractionForKeyRemoval(_ request: TeraKeyRemovalRequest) async throws {
    do {
      _ = try await runtime.prepareRetractionForKeyRemoval(draftId: request.id, expectedRevision: request.revision)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }
}
