import Foundation
import TeraKitBindings

extension FfiTodayPageRecord {
  func appValue() throws -> TeraTodayPage {
    try TeraTodayPage(
      asOfUnixSeconds: asOfUnixS, items: items.map { try $0.appValue() },
      nextCursor: nextCursor, projectionGeneration: projectionGeneration
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
