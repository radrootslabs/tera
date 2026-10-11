import Foundation

struct TeraMediaCacheCleanup: Sendable, Equatable {
  let invalidatedEntries: UInt32
  let retainedCandidates: UInt32
  let remainingEntries: UInt32
  let recoveredOrphanFiles: UInt32
  let scanPending: Bool

  init(invalidatedEntries: UInt32, retainedCandidates: UInt32, remainingEntries: UInt32, recoveredOrphanFiles: UInt32 = 0, scanPending: Bool = false) {
    self.invalidatedEntries = invalidatedEntries
    self.retainedCandidates = retainedCandidates
    self.remainingEntries = remainingEntries
    self.recoveredOrphanFiles = recoveredOrphanFiles
    self.scanPending = scanPending
  }
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
