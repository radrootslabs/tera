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

  var honestSummary: String {
    switch phase {
    case .replacementPending: "Replacement saved for delivery"
    case .replacementFailed: "Replacement failed; the original remains"
    case .retractionPending: "Replacement published; retraction is pending"
    case .complete: "Revision complete"
    case .partialEffect: "Replacement published; retraction did not complete"
    case .cancelled: "Revision cancelled"
    }
  }
}
