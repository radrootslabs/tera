import Foundation

struct TeraTodayReconcileRequest: Sendable {
  let context: TeraLocalNetwork
  let asOfUnixSeconds: UInt64
  let cardIDs: [String]
  var expectedGeneration: UInt64?
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
    asOf: UInt64, cards: [TeraTodayCard]
  ) async throws -> TeraTodayPage {
    let identifiers = cards.map(\.id)
    var current: [String: TeraTodayCard] = [:]
    var generation: UInt64?
    for offset in stride(from: 0, to: max(identifiers.count, 1), by: 100) {
      try Task.checkCancellation()
      let end = min(offset + 100, identifiers.count)
      let page = try await client.reconcileToday(request: TeraTodayReconcileRequest(
        context: context, asOfUnixSeconds: asOf,
        cardIDs: Array(identifiers[offset ..< end]), expectedGeneration: generation
      ))
      guard let received = page.projectionGeneration,
        generation == nil || generation == received
      else { throw TeraTodayReconciliationError.changed }
      generation = received
      for card in page.items {
        guard identifiers[offset ..< end].contains(card.id),
          current.updateValue(card, forKey: card.id) == nil
        else { throw TeraTodayReconciliationError.changed }
      }
    }
    return TeraTodayPage(
      asOfUnixSeconds: asOf, items: identifiers.compactMap { current[$0] },
      nextCursor: nil, projectionGeneration: generation
    )
  }
}

enum TeraTodayReconciliationError: Error {
  case changed
}
