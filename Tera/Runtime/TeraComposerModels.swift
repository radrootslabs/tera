import Foundation

/// Stable account and local context, independent of runtime generation and relay settings.
struct TeraComposerScope: Sendable, Equatable, Hashable {
  let authorPublicKey: String
  let localNetworkID: String
}

/// Reference metadata only. Loading this value never creates a file or upload-ready handle.
struct TeraComposerMedia: Sendable, Equatable, Hashable {
  var opaqueReference: String
  var sha256: String
  var mediaType: String
  var byteSize: UInt64
  var width: UInt32
  var height: UInt32
  var alt: String
  var preparedAtUnixSeconds: UInt64
}

struct TeraComposerForm: Sendable, Equatable, Hashable {
  var commandType: TeraAddCommandType
  var content: String = ""
  var identifier: String?
  var title: String?
  var summary: String?
  var location: String?
  var eventTiming: TeraEventTiming?
  var eventStartDate: String?
  var eventEndDate: String?
  var eventStartUnixSeconds: UInt64?
  var eventEndUnixSeconds: UInt64?
  var eventTimezone: String?
  var priceAmount: String?
  var currency: String?
  var unit: String?
  var quantity: String?
  var foodPublishedAtUnixSeconds: UInt64?
  var foodStatus: String?
  var media: [TeraComposerMedia] = []

  init(commandType: TeraAddCommandType) {
    self.commandType = commandType
  }

  init(editing form: TeraAddForm) {
    commandType = form.commandType
    content = form.content
    identifier = form.identifier
    title = form.title
    summary = form.summary
    location = form.location
    eventTiming = form.eventTiming
    eventStartDate = form.eventStartDate
    eventEndDate = form.eventEndDate
    eventStartUnixSeconds = form.eventStartUnixSeconds
    eventEndUnixSeconds = form.eventEndUnixSeconds
    eventTimezone = form.eventTimezone
    priceAmount = form.priceAmount
    currency = form.currency
    unit = form.unit
    quantity = form.quantity
    foodPublishedAtUnixSeconds = form.foodPublishedAtUnixSeconds
    foodStatus = form.foodStatus
    media = form.media.map { value in
      TeraComposerMedia(
        opaqueReference: value.opaqueReference,
        sha256: value.sha256,
        mediaType: value.mediaType,
        byteSize: value.byteSize,
        width: value.width,
        height: value.height,
        alt: value.alt,
        preparedAtUnixSeconds: value.preparedAtUnixSeconds
      )
    }
  }
}

struct TeraComposerSaveRequest: Sendable, Equatable {
  let scope: TeraComposerScope
  let id: String
  let expectedRevision: UInt64?
  let editSequence: UInt64
  let form: TeraComposerForm
}

struct TeraComposerDraft: Sendable, Equatable {
  let scope: TeraComposerScope
  let id: String
  let revision: UInt64
  let editSequence: UInt64
  let form: TeraComposerForm
}

/// Acknowledges this exact historical revision; newer edits require their own receipt.
struct TeraComposerSaveReceipt: Sendable, Equatable {
  let draft: TeraComposerDraft
  let replayed: Bool
}

struct TeraComposerSummary: Sendable, Equatable {
  let id: String
  let revision: UInt64
  let editSequence: UInt64
  let commandType: TeraAddCommandType
  let createdAtUnixMilliseconds: UInt64
  let updatedAtUnixMilliseconds: UInt64
}

enum TeraComposerRepairReason: Sendable, Equatable {
  case unsupportedSchema
  case corruptRecord
}

enum TeraComposerListEntry: Sendable, Equatable {
  case draft(TeraComposerSummary)
  /// This locator may contain invalid ID bytes and must never become an editing ID.
  case repair(draftKey: String, revision: UInt64, reason: TeraComposerRepairReason)
}

struct TeraComposerPage: Sendable, Equatable {
  let scope: TeraComposerScope
  let entries: [TeraComposerListEntry]
  let nextCursor: String?
}
