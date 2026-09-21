import Foundation

struct TeraRevisionTarget: Sendable, Equatable, Hashable {
  let cardID: String
  let sourceEventID: String
  let sourceAddress: String?
  let authorPublicKey: String
}

enum TeraRevisionPolicy: Sendable, Equatable, Hashable {
  case replaceThenRetract
  case addressableReplacement
}

enum TeraRevisionPhase: Sendable, Equatable, Hashable {
  case replacementPending
  case replacementFailed
  case retractionPending
  case complete
  case partialEffect
  case cancelled
}

struct TeraRevisionStatus: Sendable, Equatable {
  let operationID: String
  let replacement: TeraDraftStatus
  let retraction: TeraDraftStatus?
  let policy: TeraRevisionPolicy
  let phase: TeraRevisionPhase

  var original: TeraRevisionTarget?
  var replacementProgress: TeraRevisionBranchStatus = .unavailable
  var retractionProgress: TeraRevisionBranchStatus?
  var canResume = false
  var canCancel = false

  var honestSummary: String {
    switch phase {
    case .replacementPending: "Replacement saved for delivery"
    case .replacementFailed: "Replacement needs attention; no relay acceptance is confirmed"
    case .retractionPending: "Replacement requirements met; retraction is pending"
    case .complete: "Saved relay requirements met"
    case .partialEffect: "Revision has partial or uncertain relay outcomes"
    case .cancelled: "Local revision work stopped"
    }
  }
}

struct TeraRevisionBranchStatus: Sendable, Equatable {
  let stopped: Bool
  let canResume: Bool
  let canCancel: Bool
  let targets: TeraPublicationTargets?

  static let unavailable = Self(stopped: false, canResume: false, canCancel: false, targets: nil)
}
