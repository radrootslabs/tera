import Foundation
import TeraKitBindings

enum TeraPublicationDeliveryState: Sendable, Equatable {
  case notIssued, unknown, partiallyAccepted, accepted
}

/// A projection of backend facts, independent of the native waiter's state.
struct TeraPublicationEvidence: Sendable, Equatable {
  let state: TeraPublicationDeliveryState
  let stopRequestedAtUnixMilliseconds: UInt64?
  let schedulingRevision: UInt64
  let retainedFacts: UInt32
  let recordedAttempts: UInt32
  let unresolvedClaims: Bool

  var isStopped: Bool {
    stopRequestedAtUnixMilliseconds != nil
  }

  func follows(_ previous: Self) -> Bool {
    guard schedulingRevision >= previous.schedulingRevision,
          retainedFacts >= previous.retainedFacts, recordedAttempts >= previous.recordedAttempts,
          previous.stopRequestedAtUnixMilliseconds == nil
            || stopRequestedAtUnixMilliseconds == previous.stopRequestedAtUnixMilliseconds else { return false }
    switch previous.state {
    case .accepted: return state == .accepted
    case .partiallyAccepted: return [.partiallyAccepted, .accepted].contains(state)
    case .unknown: return state != .notIssued
    case .notIssued: return true
    }
  }

  var stoppedSummary: String {
    switch state {
    case .notIssued: "Stopped. No relay delivery attempt was issued."
    case .unknown: "Stopped. Relay delivery is uncertain; the original operation is retained for reconciliation."
    case .partiallyAccepted: "Stopped. Some saved relays accepted this publication."
    case .accepted: "Stopped. Delivery was accepted under the saved relay policy."
    }
  }

  static func decode(_ value: FfiPublicationDeliveryEvidence) throws -> Self {
    guard value.schedulingRevision > 0, value.schedulingRevision <= UInt64(Int64.max),
          value.retainedFacts <= 1024, value.recordedAttempts <= 1024,
          value.stopRequestedAtUnixMs.map({ $0 > 0 && $0 <= UInt64(Int64.max) }) != false,
          value.state != .notIssued || (!value.unresolvedClaims && value.retainedFacts == 0 && value.recordedAttempts == 0)
    else { throw TeraGeneratedSubmission.mismatch() }
    let state: TeraPublicationDeliveryState = switch value.state {
    case .notIssued: .notIssued
    case .unknown: .unknown
    case .partiallyAccepted: .partiallyAccepted
    case .accepted: .accepted
    }
    return Self(state: state, stopRequestedAtUnixMilliseconds: value.stopRequestedAtUnixMs,
                schedulingRevision: value.schedulingRevision, retainedFacts: value.retainedFacts,
                recordedAttempts: value.recordedAttempts, unresolvedClaims: value.unresolvedClaims)
  }
}
