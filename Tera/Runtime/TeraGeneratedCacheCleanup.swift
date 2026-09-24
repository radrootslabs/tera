import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  func cleanupMediaCache(context: TeraLocalNetwork) async throws -> TeraMediaCacheCleanup {
    do {
      let result = try await runtime.phase1CleanupMediaCache(context: context.generatedValue)
      return TeraMediaCacheCleanup(invalidatedEntries: result.invalidatedEntries, retainedCandidates: result.retainedCandidates, remainingEntries: result.remainingEntries)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }
}
