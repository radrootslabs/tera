struct TeraAddStartupSnapshot {
  let schemas: [TeraAddSchema]
  let drafts: [TeraDraftStatus]
  let support: TeraAddMediaSupport

  @MainActor
  static func load(
    client: TeraRuntimeClient, support: @MainActor () async throws -> TeraAddMediaSupport
  ) async throws -> Self {
    async let schemaResult = client.addSchemas()
    async let draftResult = client.draftHeads(limit: 100)
    async let supportResult = support()
    return try await Self(
      schemas: TeraProductSurfaceContract.validate(schemas: schemaResult),
      drafts: draftResult, support: supportResult
    )
  }
}
