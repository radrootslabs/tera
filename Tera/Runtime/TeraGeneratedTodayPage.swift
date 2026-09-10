import Foundation
import TeraKitBindings

extension FfiTodayPageRecord {
  var appValue: TeraTodayPage {
    TeraTodayPage(
      asOfUnixSeconds: asOfUnixS,
      items: items.map(\.appValue),
      nextCursor: nextCursor,
      projectionGeneration: projectionGeneration
    )
  }
}

extension FfiTodayCardType {
  var appValue: TeraTodayCardType {
    switch self {
    case .update: .update
    case .photoUpdate: .photoUpdate
    case .ask: .ask
    case .event: .event
    case .foodAvailability: .foodAvailability
    }
  }
}
