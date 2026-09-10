import Foundation
import TeraKitBindings

/// Gregorian components, independent of timezone or a Foundation instant.
struct TeraCivilDate: Sendable, Equatable, Hashable, Comparable {
  let year: UInt16
  let month: UInt8
  let day: UInt8

  init(year: UInt16, month: UInt8, day: UInt8) throws {
    guard (1 ... 9999).contains(year), (1 ... 12).contains(month) else {
      throw TeraCalendarTiming.unsupported
    }
    let leap = year.isMultiple(of: 4) && (!year.isMultiple(of: 100) || year.isMultiple(of: 400))
    let lengths: [UInt8] = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    guard (1 ... lengths[Int(month) - 1]).contains(day) else {
      throw TeraCalendarTiming.unsupported
    }
    self.year = year
    self.month = month
    self.day = day
  }

  var canonical: String {
    String(format: "%04d-%02d-%02d", Int(year), Int(month), Int(day))
  }

  static func < (lhs: Self, rhs: Self) -> Bool {
    (lhs.year, lhs.month, lhs.day) < (rhs.year, rhs.month, rhs.day)
  }
}

enum TeraCalendarTiming: Sendable, Equatable, Hashable {
  case dateBased(start: TeraCivilDate, endExclusive: TeraCivilDate?)
  case timeBased(
    startUnixSeconds: UInt64, endExclusiveUnixSeconds: UInt64?,
    startTimezoneID: String?, endTimezoneID: String?
  )

  static var unsupported: TeraRuntimeFailure {
    TeraRuntimeFailure(
      schemaVersion: 1, code: "today_reader_unsupported", category: "today", retryable: false,
      recoveryActions: ["upgrade_application"], operationID: nil, capabilityID: nil,
      safeMessage: "Calendar data requires a supported application version."
    )
  }

  /// The wire domain is wider than native date presentation. Keep the original
  /// UInt64 in the model and check the Gregorian display domain before conversion.
  static func presentationInstant(_ seconds: UInt64) -> Date? {
    guard seconds <= 253_402_300_799 else { return nil }
    return Date(timeIntervalSince1970: TimeInterval(seconds))
  }
}

extension FfiCivilDate {
  func appValue() throws -> TeraCivilDate {
    try TeraCivilDate(year: year, month: month, day: day)
  }
}

extension FfiCalendarTiming {
  func appValue() throws -> TeraCalendarTiming {
    switch self {
    case let .dateBased(start, endExclusive):
      let first = try start.appValue()
      let end = try endExclusive?.appValue()
      guard end == nil || end.map({ $0 > first }) == true else {
        throw TeraCalendarTiming.unsupported
      }
      return .dateBased(start: first, endExclusive: end)
    case let .timeBased(startUnixS, endExclusiveUnixS, startTzid, endTzid):
      guard endExclusiveUnixS == nil || endExclusiveUnixS.map({ $0 > startUnixS }) == true,
            [startTzid, endTzid].compactMap(\.self).allSatisfy({
              $0.utf8.count <= 255 && TimeZone(identifier: $0) != nil
            })
      else { throw TeraCalendarTiming.unsupported }
      return .timeBased(
        startUnixSeconds: startUnixS, endExclusiveUnixSeconds: endExclusiveUnixS,
        startTimezoneID: startTzid, endTimezoneID: endTzid
      )
    }
  }
}
