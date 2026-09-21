import Foundation

struct TeraDraftStatus: Sendable, Equatable, Hashable, Identifiable {
  let id: String
  let revision: UInt64
  let authorPublicKey: String
  let kind: TeraDraftKind
  let commandType: TeraAddCommandType
  let form: TeraAddForm?
  let state: TeraOutboxState
  let cardID: String
  let operationID: String?
  let createdAtUnixMilliseconds: UInt64
  let updatedAtUnixMilliseconds: UInt64
  let media: [TeraDraftMediaStatus]
  let settlement: TeraOperationSettlement?
  let isRevision: Bool
  var revisionParentID: String?
  var coordinateWritable = true
  var coordinateCaptured = false

  var canAdvance: Bool {
    coordinateWritable && state.canAdvance
  }

  var canQueue: Bool {
    coordinateWritable && (state.isEditable || state == .readyToSign)
  }

  var isEditable: Bool {
    !coordinateCaptured && state.isEditable
  }

  var honestSummary: String {
    if !coordinateWritable, ![.complete, .cancelled, .terminal].contains(state) {
      return TeraPublicationActionReason.coordinateChanged.explanation
    }
    if isRevision {
      return "Saved revision; open Drafts & outbox for current relay outcomes."
    }
    if revisionParentID != nil {
      return "Retraction belongs to a saved revision."
    }
    return state.summary(settlement: settlement)
  }
}
