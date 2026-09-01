import Foundation

enum RadrootsClockError: Error, Sendable, Equatable {
  case nonfinite
  case beforeUnixEpoch
  case overflow
  case zeroNotAllowed
}

struct RadrootsClock: Sendable {
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
      throw RadrootsClockError.overflow
    }
    return signed
  }

  private static func unsigned(
    _ seconds: TimeInterval,
    multiplier: TimeInterval,
    requirePositive: Bool
  ) throws -> UInt64 {
    guard seconds.isFinite else {
      throw RadrootsClockError.nonfinite
    }
    guard seconds >= 0 else {
      throw RadrootsClockError.beforeUnixEpoch
    }
    let scaled = seconds * multiplier
    guard scaled.isFinite,
      let value = UInt64(exactly: scaled.rounded(.down))
    else {
      throw RadrootsClockError.overflow
    }
    guard !requirePositive || value > 0 else {
      throw RadrootsClockError.zeroNotAllowed
    }
    return value
  }
}

enum RadrootsStateTransitionError: Error, Sendable, Equatable {
  case generationOverflow
}

enum RadrootsCheckedStateTransition {
  static func nextGeneration(after generation: UInt64) throws -> UInt64 {
    let (next, overflow) = generation.addingReportingOverflow(1)
    guard !overflow else {
      throw RadrootsStateTransitionError.generationOverflow
    }
    return next
  }
}
