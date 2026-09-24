import Foundation

struct TeraMediaCacheCleanup: Sendable, Equatable {
  let invalidatedEntries: UInt32
  let retainedCandidates: UInt32
  let remainingEntries: UInt32
}

extension TeraRuntimeBackend {
  func cleanupMediaCache(context _: TeraLocalNetwork) async throws -> TeraMediaCacheCleanup {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}

extension TeraRuntimeClient {
  func cleanupMediaCache(context: TeraLocalNetwork) async throws -> TeraMediaCacheCleanup {
    try await supportOperation("runtime.media.cleanup") { try await $0.cleanupMediaCache(context: context) }
  }
}
