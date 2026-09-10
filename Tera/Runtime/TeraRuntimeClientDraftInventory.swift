import Foundation

extension TeraRuntimeClient {
  func legacyDraftPage(limit: UInt16 = 100, cursor: String? = nil) async throws -> TeraLegacyDraftPage {
    try await addOperation("runtime.draftInventory") { backend in
      try await backend.legacyDraftPage(limit: limit, cursor: cursor)
    }
  }
}
