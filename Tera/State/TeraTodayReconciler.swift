import Foundation

struct TeraTodayReconcileRequest: Sendable {
  let context: TeraLocalNetwork
  let asOfUnixSeconds: UInt64
  let cardIDs: [String]
  var expectedGeneration: UInt64?
  let calendar: TeraViewerCalendarContext
}

enum TeraTodayReconciler {
  static func unique<Value: Identifiable>(_ values: [Value]) -> [Value] {
    var identifiers = Set<Value.ID>()
    return values.filter { identifiers.insert($0.id).inserted }
  }

  /// Sequential batches share one generation. No changed snapshot is partially
  /// installed and no newly discovered identity is inserted into a scrolled feed.
  static func read(
    client: TeraRuntimeClient, context: TeraLocalNetwork,
    calendar: TeraViewerCalendarContext, cards: [TeraTodayCard]
  ) async throws -> TeraTodayPage {
    let identifiers = cards.map(\.id)
    var current: [String: TeraTodayCard] = [:]
    var generation: UInt64?
    for offset in stride(from: 0, to: max(identifiers.count, 1), by: 100) {
      try Task.checkCancellation()
      let end = min(offset + 100, identifiers.count)
      let page = try await client.reconcileToday(request: TeraTodayReconcileRequest(
        context: context, asOfUnixSeconds: calendar.asOfUnixSeconds,
        cardIDs: Array(identifiers[offset ..< end]), expectedGeneration: generation, calendar: calendar
      ))
      guard let received = page.projectionGeneration,
        generation == nil || generation == received, page.calendar == calendar,
        page.asOfUnixSeconds == calendar.asOfUnixSeconds
      else { throw TeraTodayReconciliationError.changed }
      generation = received
      for card in page.items {
        guard identifiers[offset ..< end].contains(card.id),
          current.updateValue(card, forKey: card.id) == nil
        else { throw TeraTodayReconciliationError.changed }
      }
    }
    return TeraTodayPage(
      asOfUnixSeconds: calendar.asOfUnixSeconds, items: identifiers.compactMap { current[$0] },
      nextCursor: nil, projectionGeneration: generation, calendar: calendar
    )
  }
}

enum TeraTodayReconciliationError: Error {
  case changed
}
