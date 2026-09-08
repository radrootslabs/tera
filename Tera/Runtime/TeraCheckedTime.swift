import Foundation

enum TeraClockError: Error, Sendable, Equatable {
  case nonfinite
  case beforeUnixEpoch
  case overflow
  case zeroNotAllowed
}

struct TeraClock: Sendable {
  private let now: @Sendable () -> Date

  static let system = Self(now: { Date() })

  init(now: @escaping @Sendable () -> Date) {
    self.now = now
  }

  static func fixed(unixSeconds: UInt64) -> Self {
    Self(now: { Date(timeIntervalSince1970: TimeInterval(unixSeconds)) })
  }

  func unixSeconds(requirePositive: Bool = false) throws -> UInt64 {
    try Self.unixSeconds(from: now(), requirePositive: requirePositive)
  }

  func unixMilliseconds(requirePositive: Bool = false) throws -> UInt64 {
    try Self.unixMilliseconds(from: now(), requirePositive: requirePositive)
  }

  static func unixSeconds(
    from date: Date,
    requirePositive: Bool = false
  ) throws -> UInt64 {
    try unsigned(date.timeIntervalSince1970, multiplier: 1, requirePositive: requirePositive)
  }

  static func unixMilliseconds(
    from date: Date,
    requirePositive: Bool = false
  ) throws -> UInt64 {
    try unsigned(date.timeIntervalSince1970, multiplier: 1000, requirePositive: requirePositive)
  }

  static func signedUnixMilliseconds(from date: Date) throws -> Int64 {
    let value = try unixMilliseconds(from: date)
    guard let signed = Int64(exactly: value) else {
      throw TeraClockError.overflow
    }
    return signed
  }

  private static func unsigned(
    _ seconds: TimeInterval,
    multiplier: TimeInterval,
    requirePositive: Bool
  ) throws -> UInt64 {
    guard seconds.isFinite else {
      throw TeraClockError.nonfinite
    }
    guard seconds >= 0 else {
      throw TeraClockError.beforeUnixEpoch
    }
    let scaled = seconds * multiplier
    guard scaled.isFinite,
      let value = UInt64(exactly: scaled.rounded(.down))
    else {
      throw TeraClockError.overflow
    }
    guard !requirePositive || value > 0 else {
      throw TeraClockError.zeroNotAllowed
    }
    return value
  }
}

enum TeraStateTransitionError: Error, Sendable, Equatable {
  case generationOverflow
}

enum TeraCheckedStateTransition {
  static func nextGeneration(after generation: UInt64) throws -> UInt64 {
    let (next, overflow) = generation.addingReportingOverflow(1)
    guard !overflow else {
      throw TeraStateTransitionError.generationOverflow
    }
    return next
  }
}
