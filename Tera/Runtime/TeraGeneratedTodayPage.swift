import Foundation
import TeraKitBindings

extension FfiTodayPageRecord {
  func appValue() throws -> TeraTodayPage {
    let viewer = try calendar.appValue()
    guard schemaVersion == 2, viewer.asOfUnixSeconds == asOfUnixS else {
      throw TeraCalendarTiming.unsupported
    }
    return try TeraTodayPage(
      asOfUnixSeconds: asOfUnixS, items: items.map { try $0.appValue() },
      nextCursor: nextCursor, projectionGeneration: projectionGeneration, calendar: viewer
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
