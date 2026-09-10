import Foundation

enum TeraDraftRecoverySelection {
  case composer(String)
  case legacy(String)
}

enum TeraRecoveredDraft {
  case composer(TeraComposerDraft)
  case legacy(TeraDraftStatus)
}

/// Metadata for an author-owned legacy operation. It carries no composer context.
struct TeraLegacyDraftSummary: Sendable, Equatable, Identifiable {
  let id: String
  let revision: UInt64
  let kind: TeraDraftKind
  let commandType: TeraAddCommandType
  let state: TeraOutboxState
  let hasForm: Bool
  let isRevision: Bool
  let createdAtUnixMilliseconds: UInt64
  let updatedAtUnixMilliseconds: UInt64
  let mediaCount: UInt64
  let verifiedMediaCount: UInt64
  let possibleOrphanCount: UInt64
  let settlement: TeraOperationSettlement?

  var honestSummary: String {
    settlement?.summary ?? state.label
  }

  var mediaSummary: String {
    let verified = "\(verifiedMediaCount) of \(mediaCount) photos verified"
    return possibleOrphanCount > 0 ? "\(verified); \(possibleOrphanCount) possible orphan" : verified
  }
}

enum TeraLegacyDraftRepairReason: Sendable, Equatable {
  case unsupportedSchema
  case corruptRecord
  case needsAttention
}

enum TeraLegacyDraftListEntry: Sendable, Equatable {
  case draft(TeraLegacyDraftSummary)
  case repair(draftKey: String, revision: UInt64, reason: TeraLegacyDraftRepairReason)
}

struct TeraLegacyDraftPage: Sendable, Equatable {
  let authorPublicKey: String
  let entries: [TeraLegacyDraftListEntry]
  let nextCursor: String?
}

extension TeraComposerForm {
  /// Rehydrates editing metadata only; a reference is never an open file or
  /// proof that an upload exists. The media owner must reopen and validate it.
  var editingValue: TeraAddForm {
    var form = TeraAddForm(commandType: commandType)
    form.content = content
    form.identifier = identifier
    form.title = title
    form.summary = summary
    form.location = location
    form.eventTiming = eventTiming
    form.eventStartDate = eventStartDate
    form.eventEndDate = eventEndDate
    form.eventStartUnixSeconds = eventStartUnixSeconds
    form.eventEndUnixSeconds = eventEndUnixSeconds
    form.eventTimezone = eventTimezone
    form.priceAmount = priceAmount
    form.currency = currency
    form.unit = unit
    form.quantity = quantity
    form.foodPublishedAtUnixSeconds = foodPublishedAtUnixSeconds
    form.foodStatus = foodStatus
    form.media = media.map { value in
      TeraPreparedMedia(opaqueReference: value.opaqueReference, remoteURL: nil,
                        sha256: value.sha256, mediaType: value.mediaType, byteSize: value.byteSize,
                        width: value.width, height: value.height, alt: value.alt,
                        preparedAtUnixSeconds: value.preparedAtUnixSeconds)
    }
    return form
  }
}
