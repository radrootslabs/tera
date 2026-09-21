import Foundation
import TeraKitBindings

enum TeraPublicationActionReason: Sendable, Equatable {
  case coordinateChanged
  case deadlineExceeded, attemptLimit, authenticationRequired, quotaExceeded
  case invalidPayload, deliveryRefused

  var explanation: String {
    switch self {
    case .coordinateChanged: "Another saved request owns this address, or its known revision changed. Review the current revision before publishing. Your captured form and any signed evidence are retained."
    case .deadlineExceeded: "The saved delivery window does not allow another attempt. The captured form and any signed publication or relay evidence are retained. Review them before choosing another submission."
    case .attemptLimit: "The delivery attempt limit was reached. The saved publication and relay evidence are retained for review."
    case .authenticationRequired: "A saved relay requires authentication. Review its access requirements before authorizing further publication. This request is retained."
    case .quotaExceeded: "A saved relay refused this publication because its quota was exhausted. Review that relay's limits. This request and its recorded effects are retained."
    case .invalidPayload: "A saved relay reported a malformed publication. Review the captured form and relay evidence. The original signed publication is retained."
    case .deliveryRefused: "A saved relay refused the publication. Review its requirements and the captured form. Recorded remote effects are retained."
    }
  }
}

/// Translation and presentation only; Rust decides whether new work may start.
enum TeraPublicationRetry: Sendable, Equatable {
  case ready, complete, stopped
  case deferredUntil(UInt64), inFlightUntil(UInt64)
  case needsAction(TeraPublicationActionReason)

  var mayStart: Bool {
    self == .ready
  }

  var explanation: String? {
    switch self {
    case .ready, .complete, .stopped: nil
    case let .needsAction(reason): reason.explanation
    case let .deferredUntil(at): "The original publication is saved for retry. Check its status after \(date(at))."
    case let .inFlightUntil(at): "An original delivery attempt is still claimed. Check its status after \(date(at)); recorded effects remain available."
    }
  }

  static func decode(_ value: FfiPublicationRetryDecision) throws -> Self {
    switch value {
    case .ready: .ready
    case .complete: .complete
    case .stopped: .stopped
    case let .deferredUntil(at): try .deferredUntil(timestamp(at))
    case let .inFlightUntil(at): try .inFlightUntil(timestamp(at))
    case let .needsAction(reason): .needsAction(decode(reason))
    }
  }

  private static func decode(_ reason: FfiPublicationActionReason) -> TeraPublicationActionReason {
    switch reason {
    case .coordinateChanged: .coordinateChanged
    case .deadlineExceeded: .deadlineExceeded
    case .attemptLimit: .attemptLimit
    case .authenticationRequired: .authenticationRequired
    case .quotaExceeded: .quotaExceeded
    case .invalidPayload: .invalidPayload
    case .deliveryRefused: .deliveryRefused
    }
  }

  private static func timestamp(_ value: UInt64) throws -> UInt64 {
    guard value > 0, value <= UInt64(Int64.max) else { throw TeraGeneratedSubmission.mismatch() }
    return value
  }

  private func date(_ value: UInt64) -> String {
    Date(timeIntervalSince1970: Double(value) / 1000).formatted(date: .abbreviated, time: .standard)
  }
}
