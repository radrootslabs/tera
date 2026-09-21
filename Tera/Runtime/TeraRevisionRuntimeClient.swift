import Foundation

extension TeraRuntimeClient {
  func saveRevisionIntent(
    requestID: String,
    target: TeraRevisionTarget,
    replacement: TeraAddRuntimeInput
  ) async throws -> TeraRevisionStatus {
    try await addOperation("runtime.add.revision.save") { backend in
      try await backend.saveRevisionIntent(requestID: requestID, target: target, replacement: replacement)
    }
  }

  func revisionSourceForm(card: TeraTodayCard, sourceDraftID: String) async throws -> TeraAddForm {
    try await addOperation("runtime.add.revision.source") { backend in
      try await backend.revisionSourceForm(card: card, sourceDraftID: sourceDraftID)
    }
  }

  func revisionStatus(operationID: String) async throws -> TeraRevisionStatus {
    try await addOperation("runtime.add.revision.status") { backend in
      try await backend.revisionStatus(operationID: operationID)
    }
  }

  func advanceRevision(operationID: String) async throws -> TeraRevisionStatus {
    try await addOperation("runtime.add.revision.advance") { backend in
      try await backend.advanceRevision(operationID: operationID)
    }
  }

  func cancelRevision(operationID: String) async throws -> TeraRevisionStatus {
    try await addOperation("runtime.add.revision.cancel") { backend in
      try await backend.cancelRevision(operationID: operationID)
    }
  }
}
