struct TeraAddStartupSnapshot {
  let schemas: [TeraAddSchema]
  let drafts: [TeraDraftStatus]
  let support: TeraAddMediaSupport
  let mediaMessage: String?

  @MainActor
  static func load(
    client: TeraRuntimeClient, support: @MainActor () async throws -> TeraAddMediaSupport,
    ready: @MainActor ([TeraAddSchema]) -> Void
  ) async throws -> Self {
    let schemas = try await TeraProductSurfaceContract.validate(schemas: client.addSchemas())
    // Presentation and scoped editing recovery start before optional legacy or
    // media work. A media failure cannot make a valid composer unavailable.
    ready(schemas)
    async let draftResult = try? client.draftHeads(limit: 100)
    async let supportResult = try? support()
    let (drafts, support) = await (draftResult, supportResult)
    return Self(schemas: schemas, drafts: drafts ?? [], support: support ?? .unavailable,
                mediaMessage: support == nil ? "Photo support is unavailable. Saved editing is still available." : nil)
  }

  static func reconcile(media: (any TeraAddMediaHandling)?, drafts: [TeraDraftStatus]) async -> String? {
    do {
      try await media?.reconcileBackgroundUploads(drafts: drafts)
      return nil
    } catch {
      return "Photo recovery needs attention. Saved editing is still available."
    }
  }
}
