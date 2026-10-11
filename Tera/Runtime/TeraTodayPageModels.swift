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

struct TeraTodayCard: Sendable, Equatable, Hashable, Identifiable {
  let id: String
  let type: TeraTodayCardType
  let sourceEventID: String
  let sourceAddress: String?
  let authorPublicKey: String
  let contractID: String
  let title: String?
  let content: String
  let authoredAtUnixSeconds: UInt64
  let effectiveAtUnixSeconds: UInt64
  let calendarTiming: TeraCalendarTiming?
  let location: String?
  let priceAmount: String?
  let priceCurrency: String?
  let priceUnit: String?
  let quantity: String?
  let foodSummary: String?
  let foodPublishedAtUnixSeconds: UInt64?
  let foodStatus: String?
  let contextRank: UInt8
  let inclusionReason: String
  let media: [TeraMediaReference]
  let lifecycle: TeraCardLifecycleState
  let rankDigest: String?
  let authorProfile: TeraProfileSummary?
  let thread: [TeraThreadEntry]
  let threadNextCursor: String?
  var localSourceDraftID: String?
  let localOperationID: String?
  let localOperationState: String?

  var authorName: String {
    authorProfile?.preferredName
      ?? TeraProfileSummary(
        authorPublicKey: authorPublicKey,
        name: nil,
        displayName: nil,
        about: nil,
        picture: nil,
        banner: nil,
        nip05: nil,
        website: nil,
        lightningAddress: nil
      ).preferredName
  }

  var priceSummary: String? {
    guard let priceAmount, let priceCurrency, let priceUnit else { return nil }
    return "\(priceAmount) \(priceCurrency)/\(priceUnit)"
  }

  var retractionTargetKind: UInt32? {
    switch type {
    case .update, .photoUpdate, .ask:
      guard sourceAddress == nil else { return nil }
      return 1
    case .event:
      guard let kind = sourceAddressKind, kind == 31922 || kind == 31923 else { return nil }
      return kind
    case .foodAvailability:
      guard sourceAddressKind == 30402 else { return nil }
      return 30402
    }
  }

  private var sourceAddressKind: UInt32? {
    guard let sourceAddress,
      let rawKind = sourceAddress.split(separator: ":", maxSplits: 1).first,
      let kind = UInt32(rawKind)
    else {
      return nil
    }
    return kind
  }
}
