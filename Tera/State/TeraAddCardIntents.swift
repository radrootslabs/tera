import Foundation

/// Existing native card-to-intent translation. Rust retains authorization,
/// validation, immutable operation identity and all publication policy.
enum TeraAddCardIntents {
  struct RevisionEditing {
    let target: TeraRevisionTarget
    let form: TeraAddForm
  }

  struct Retraction {
    let id: String
    let input: TeraRetractionDraftInput
  }

  static func revision(_ card: TeraTodayCard, author: String?, client: TeraRuntimeClient) async throws -> RevisionEditing {
    guard let author, author == card.authorPublicKey else {
      throw TeraRuntimeFailure.local(operation: "add.revise", code: "ios.add.revision_not_authorized",
                                     safeMessage: "Only your own post can be revised.")
    }
    guard let id = card.localSourceDraftID ?? card.localOperationID else {
      throw TeraRuntimeFailure.local(operation: "add.revise", code: "ios.add.revision_source_unavailable",
                                     safeMessage: "This post cannot be revised losslessly on this device.")
    }
    let form = try await client.revisionSourceForm(card: card, sourceDraftID: id)
    return RevisionEditing(target: TeraRevisionTarget(cardID: card.id, sourceEventID: card.sourceEventID,
                                                      sourceAddress: card.sourceAddress, authorPublicKey: card.authorPublicKey), form: form)
  }

  static func retraction(_ card: TeraTodayCard, author: String?, identifier: () -> String) throws -> Retraction {
    guard let author, author == card.authorPublicKey else {
      throw TeraRuntimeFailure.local(operation: "add.retract", code: "ios.add.retraction_not_authorized",
                                     safeMessage: "Only your own post can be retracted.")
    }
    guard let kind = card.retractionTargetKind else {
      throw TeraRuntimeFailure.local(operation: "add.retract", code: "ios.add.retraction_target_invalid",
                                     safeMessage: "This post cannot be retracted safely.")
    }
    let id = identifier()
    guard TeraAddPresentation.isValidIdentifier(id) else {
      throw TeraRuntimeFailure.local(operation: "add.retract", code: "ios.add.identifier_invalid",
                                     safeMessage: "The local operation identifier is invalid.")
    }
    return Retraction(id: id, input: TeraRetractionDraftInput(commandType: card.type.addCommandType,
                                                              targetCardID: card.id, targetEventID: card.sourceEventID,
                                                              targetKind: kind, targetAddress: card.sourceAddress, reason: "Removed by author."))
  }
}
