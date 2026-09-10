import TeraKitBindings

enum TeraGeneratedTodayReads {
  static func page(
    runtime: TeraRuntime, request: TeraTodayPageRequest
  ) async throws -> TeraTodayPage {
    try await runtime.phase1TodayPage(
      context: request.context.generatedValue, limit: request.limit,
      asOfUnixS: request.asOfUnixSeconds, cursor: request.cursor,
      viewerTimeZone: request.viewerTimeZone
    ).appValue()
  }

  static func reconcile(
    runtime: TeraRuntime, request: TeraTodayReconcileRequest
  ) async throws -> TeraTodayPage {
    try await runtime.phase1TodayReconcile(
      context: request.context.generatedValue, asOfUnixS: request.asOfUnixSeconds,
      cardIds: request.cardIDs, expectedGeneration: request.expectedGeneration,
      viewerTimeZone: request.calendar.timeZoneID
    ).appValue()
  }
}
