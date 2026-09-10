import Foundation
import TeraKitBindings

enum TeraGeneratedDraftInventory {
  static func page(runtime: TeraRuntime, limit: UInt16, cursor: String?) async throws -> TeraLegacyDraftPage {
    let record = try await runtime.legacyDraftPage(schemaVersion: 1, limit: limit, cursor: cursor)
    try version(record.schemaVersion)
    return try TeraLegacyDraftPage(authorPublicKey: record.authorPublicKey,
                                   entries: record.entries.map(entry), nextCursor: record.nextCursor)
  }

  private static func entry(_ value: FfiLegacyDraftListEntry) throws -> TeraLegacyDraftListEntry {
    switch value {
    case let .draft(summary):
      try version(summary.schemaVersion)
      if let settlement = summary.settlement {
        try version(settlement.schemaVersion)
      }
      return .draft(TeraLegacyDraftSummary(id: summary.draftId, revision: summary.revision,
                                           kind: summary.kind.appValue, commandType: summary.commandType.appValue,
                                           state: summary.state.appValue, hasForm: summary.hasForm,
                                           isRevision: summary.isRevision,
                                           createdAtUnixMilliseconds: summary.createdAtUnixMs,
                                           updatedAtUnixMilliseconds: summary.updatedAtUnixMs,
                                           mediaCount: summary.mediaCount, verifiedMediaCount: summary.verifiedMediaCount,
                                           possibleOrphanCount: summary.possibleOrphanCount, settlement: summary.settlement?.appValue))
    case let .repair(draftKey, revision, reason):
      let reason: TeraLegacyDraftRepairReason = switch reason {
      case .unsupportedSchema: .unsupportedSchema
      case .corruptRecord: .corruptRecord
      case .needsAttention: .needsAttention
      }
      return .repair(draftKey: draftKey, revision: revision, reason: reason)
    }
  }

  private static func version(_ value: UInt16) throws {
    guard value == 1 else {
      throw TeraRuntimeFailure.local(operation: "runtime.draftInventory", code: "unsupported_schema_version",
                                     safeMessage: "This saved draft format requires a compatible app.")
    }
  }
}

extension FfiAddCommandType {
  var appValue: TeraAddCommandType {
    switch self {
    case .createUpdate: .createUpdate
    case .createPhotoUpdate: .createPhotoUpdate
    case .createAsk: .createAsk
    case .createEvent: .createEvent
    case .createFoodAvailability: .createFoodAvailability
    }
  }
}

extension FfiDraftKind {
  var appValue: TeraDraftKind {
    switch self {
    case .add: .add
    case .retraction: .retraction
    }
  }
}

extension FfiOutboxState {
  var appValue: TeraOutboxState {
    switch self {
    case .draft: .draft
    case .mediaPreparing: .mediaPreparing
    case .mediaUploading: .mediaUploading
    case .readyToSign: .readyToSign
    case .signing: .signing
    case .signed: .signed
    case .queued: .queued
    case .delivering: .delivering
    case .partiallyDelivered: .partiallyDelivered
    case .retryable: .retryable
    case .terminal: .terminal
    case .cancelled: .cancelled
    case .complete: .complete
    }
  }
}
