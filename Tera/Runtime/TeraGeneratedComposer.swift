import Foundation
import TeraKitBindings

enum TeraGeneratedComposer {
  static func reserveID() throws -> String {
    let record = try composerReserveId()
    try version(record.schemaVersion)
    return record.id
  }

  static func save(runtime: TeraRuntime, request: TeraComposerSaveRequest) async throws -> TeraComposerSaveReceipt {
    let record = try await runtime.composerSave(request: request.generatedValue)
    try version(record.schemaVersion)
    let draft = try record.draft.composerAppValue
    guard draft.scope == request.scope, draft.id == request.id,
          draft.editSequence == request.editSequence, draft.form == request.form,
          draft.revision == (request.expectedRevision.map { $0 &+ 1 } ?? 1)
    else { throw mismatch() }
    return TeraComposerSaveReceipt(draft: draft, replayed: record.replayed)
  }

  static func load(runtime: TeraRuntime, scope: TeraComposerScope, id: String) async throws -> TeraComposerDraft {
    let draft = try await runtime.composerLoad(scope: scope.generatedValue, composerId: id).composerAppValue
    guard draft.scope == scope, draft.id == id else { throw mismatch() }
    return draft
  }

  static func list(runtime: TeraRuntime, scope: TeraComposerScope, limit: UInt16, cursor: String?) async throws -> TeraComposerPage {
    let record = try await runtime.composerList(schemaVersion: 1, scope: scope.generatedValue, limit: limit, cursor: cursor)
    try version(record.schemaVersion)
    let actualScope = try record.scope.composerAppValue
    guard actualScope == scope else { throw mismatch() }
    return try TeraComposerPage(scope: actualScope, entries: record.entries.map { entry in
      switch entry {
      case let .draft(summary):
        try version(summary.schemaVersion)
        return .draft(TeraComposerSummary(
          id: summary.id, revision: summary.revision, editSequence: summary.editSequence,
          commandType: summary.commandType.composerAppValue,
          createdAtUnixMilliseconds: summary.createdAtUnixMs,
          updatedAtUnixMilliseconds: summary.updatedAtUnixMs
        ))
      case let .repair(draftKey, revision, reason):
        return .repair(draftKey: draftKey, revision: revision,
                       reason: reason == .unsupportedSchema ? .unsupportedSchema : .corruptRecord)
      }
    }, nextCursor: record.nextCursor)
  }

  static func version(_ version: UInt16) throws {
    guard version == 1 else {
      throw TeraRuntimeFailure.local(operation: "runtime.composer", code: "composer_schema_unsupported",
                                     safeMessage: "This local composer format requires a compatible app.")
    }
  }

  private static func mismatch() -> TeraRuntimeFailure {
    .local(operation: "runtime.composer", code: "composer_receipt_mismatch",
           safeMessage: "The local composer response could not be reconciled.")
  }
}

extension TeraComposerScope {
  var generatedValue: FfiComposerScopeRecord {
    FfiComposerScopeRecord(schemaVersion: 1, authorPublicKey: authorPublicKey, localNetworkId: localNetworkID)
  }
}

extension FfiComposerScopeRecord {
  var composerAppValue: TeraComposerScope {
    get throws {
      try TeraGeneratedComposer.version(schemaVersion)
      return TeraComposerScope(authorPublicKey: authorPublicKey, localNetworkID: localNetworkId)
    }
  }
}

extension TeraComposerSaveRequest {
  var generatedValue: FfiComposerSaveRequest {
    FfiComposerSaveRequest(schemaVersion: 1, scope: scope.generatedValue, id: id,
                           expectedRevision: expectedRevision, editSequence: editSequence, form: form.generatedValue)
  }
}

extension FfiComposerDraftRecord {
  var composerAppValue: TeraComposerDraft {
    get throws {
      try TeraGeneratedComposer.version(schemaVersion)
      return try TeraComposerDraft(scope: scope.composerAppValue, id: id, revision: revision,
                                   editSequence: editSequence, form: form.composerAppValue)
    }
  }
}

extension TeraComposerForm {
  var generatedValue: FfiComposerFormRecord {
    FfiComposerFormRecord(
      schemaVersion: 1,
      commandType: commandType.composerGeneratedValue,
      content: content,
      identifier: identifier,
      title: title,
      summary: summary,
      location: location,
      eventTiming: eventTiming?.composerGeneratedValue,
      eventStartDate: eventStartDate,
      eventEndDate: eventEndDate,
      eventStartUnixS: eventStartUnixSeconds,
      eventEndUnixS: eventEndUnixSeconds,
      eventTimezone: eventTimezone,
      priceAmount: priceAmount,
      currency: currency,
      unit: unit,
      quantity: quantity,
      foodPublishedAtUnixS: foodPublishedAtUnixSeconds,
      foodStatus: foodStatus,
      media: media.map(\.generatedValue)
    )
  }
}

extension FfiComposerFormRecord {
  var composerAppValue: TeraComposerForm {
    get throws {
      try TeraGeneratedComposer.version(schemaVersion)
      var form = TeraComposerForm(commandType: commandType.composerAppValue)
      form.content = content
      form.identifier = identifier
      form.title = title
      form.summary = summary
      form.location = location
      form.eventTiming = eventTiming?.composerAppValue
      form.eventStartDate = eventStartDate
      form.eventEndDate = eventEndDate
      form.eventStartUnixSeconds = eventStartUnixS
      form.eventEndUnixSeconds = eventEndUnixS
      form.eventTimezone = eventTimezone
      form.priceAmount = priceAmount
      form.currency = currency
      form.unit = unit
      form.quantity = quantity
      form.foodPublishedAtUnixSeconds = foodPublishedAtUnixS
      form.foodStatus = foodStatus
      form.media = try media.map { try $0.composerAppValue }
      return form
    }
  }
}

extension TeraComposerMedia {
  var generatedValue: FfiComposerMediaRecord {
    FfiComposerMediaRecord(
      schemaVersion: 1,
      opaqueReference: opaqueReference,
      sha256: sha256,
      mediaType: mediaType,
      byteSize: byteSize,
      width: width,
      height: height,
      alt: alt,
      preparedAtUnixS: preparedAtUnixSeconds
    )
  }
}

extension FfiComposerMediaRecord {
  var composerAppValue: TeraComposerMedia {
    get throws {
      try TeraGeneratedComposer.version(schemaVersion)
      return TeraComposerMedia(
        opaqueReference: opaqueReference,
        sha256: sha256,
        mediaType: mediaType,
        byteSize: byteSize,
        width: width,
        height: height,
        alt: alt,
        preparedAtUnixSeconds: preparedAtUnixS
      )
    }
  }
}

extension TeraAddCommandType {
  fileprivate var composerGeneratedValue: FfiAddCommandType {
    switch self {
    case .createUpdate: .createUpdate
    case .createPhotoUpdate: .createPhotoUpdate
    case .createAsk: .createAsk
    case .createEvent: .createEvent
    case .createFoodAvailability: .createFoodAvailability
    }
  }
}

extension FfiAddCommandType {
  fileprivate var composerAppValue: TeraAddCommandType {
    switch self {
    case .createUpdate: .createUpdate
    case .createPhotoUpdate: .createPhotoUpdate
    case .createAsk: .createAsk
    case .createEvent: .createEvent
    case .createFoodAvailability: .createFoodAvailability
    }
  }
}

extension TeraEventTiming {
  fileprivate var composerGeneratedValue: FfiEventTimingKind {
    switch self {
    case .allDay: .allDay
    case .timed: .timed
    }
  }
}

extension FfiEventTimingKind {
  fileprivate var composerAppValue: TeraEventTiming {
    switch self {
    case .allDay: .allDay
    case .timed: .timed
    }
  }
}
