import Foundation

struct TeraTodayPageRequest: Sendable, Equatable {
  let context: TeraLocalNetwork
  let limit: UInt16
  let asOfUnixSeconds: UInt64?
  let cursor: String?
  let viewerTimeZone: String?

  static func first(
    context: TeraLocalNetwork,
    limit: UInt16,
    asOfUnixSeconds: UInt64,
    timeZone: TimeZone = .current
  ) -> Self {
    Self(context: context, limit: limit, asOfUnixSeconds: asOfUnixSeconds, cursor: nil, viewerTimeZone: timeZone.identifier)
  }

  static func after(context: TeraLocalNetwork, limit: UInt16, cursor: String) -> Self {
    Self(context: context, limit: limit, asOfUnixSeconds: nil, cursor: cursor, viewerTimeZone: nil)
  }
}

struct TeraTodayPage: Sendable, Equatable {
  let asOfUnixSeconds: UInt64
  let items: [TeraTodayCard]
  let nextCursor: String?
  var projectionGeneration: UInt64?
  let calendar: TeraViewerCalendarContext
}
