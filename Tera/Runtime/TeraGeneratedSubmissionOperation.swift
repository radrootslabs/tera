import Foundation
import TeraKitBindings

extension TeraSubmissionRequest {
  var generatedValue: FfiSubmissionReservationRequest {
    FfiSubmissionReservationRequest(schemaVersion: 1, commandId: commandID, scope: scope.generatedValue,
                                    composerId: composerID, expectedRevision: expectedRevision)
  }
}

extension TeraSubmissionMediaRequest {
  var generatedValue: FfiSubmissionMediaInput {
    FfiSubmissionMediaInput(schemaVersion: 1, request: request.generatedValue,
                            expectedRevision: expectedRevision, media: media.generatedValue)
  }
}

extension TeraGeneratedSubmission {
  static func request(_ value: FfiSubmissionReservationRequest) throws -> TeraSubmissionRequest {
    try version(value.schemaVersion)
    guard validID(value.commandId), validID(value.composerId), value.expectedRevision > 0,
          value.expectedRevision <= UInt64(Int64.max) else { throw mismatch() }
    return try TeraSubmissionRequest(commandID: value.commandId, scope: value.scope.composerAppValue,
                                     composerID: value.composerId, expectedRevision: value.expectedRevision)
  }

  static func operation(_ value: FfiSubmissionOperationRecord, expected: TeraSubmissionRequest) throws -> TeraSubmissionStatus {
    try version(value.schemaVersion)
    try version(value.settlement.schemaVersion)
    let actualRequest = try request(value.request)
    let captured = try value.captured.composerAppValue
    guard actualRequest == expected, validID(value.intentId), validID(value.operationId),
          value.revision > 0, value.revision <= UInt64(Int64.max),
          captured.scope == expected.scope, captured.id == expected.composerID,
          captured.revision == expected.expectedRevision, captured.editSequence > 0,
          value.committedAtUnixMs > 0, value.updatedAtUnixMs >= value.committedAtUnixMs,
          value.updatedAtUnixMs <= UInt64(Int64.max), value.media.count == captured.form.media.count,
          value.media.count <= 20 else { throw mismatch() }
    let media = try zip(value.media, captured.form.media).map { progress, source in
      guard progress.progress.schemaVersion == 2,
            progress.opaqueReference == source.opaqueReference,
            progress.progress.uploadUrl?.isEmpty == false,
            !progress.progress.url.isEmpty else { throw mismatch() }
      guard progress.authorizations.count <= 5 else { throw mismatch() }
      let attempts = try progress.authorizations.map { attempt in
        guard validID(attempt.operationId), attempt.expirationUnixS > 0,
              attempt.expirationUnixS <= UInt64(Int64.max) / 1000,
              attempt.revision.map({ $0 > 0 && $0 <= value.revision }) ?? true else { throw mismatch() }
        return TeraUploadAttemptIdentity(operationID: attempt.operationId, revision: attempt.revision,
                                         expirationUnixSeconds: attempt.expirationUnixS)
      }
      guard Set(attempts.map(\.operationID)).count == attempts.count else { throw mismatch() }
      return TeraSubmissionMedia(opaqueReference: progress.opaqueReference, progress: progress.progress.appValue, authorizations: attempts)
    }
    return try TeraSubmissionStatus(request: actualRequest, intentID: value.intentId, operationID: value.operationId,
                                    revision: value.revision, captured: captured, state: value.state.appValue,
                                    committedAtUnixMilliseconds: value.committedAtUnixMs,
                                    updatedAtUnixMilliseconds: value.updatedAtUnixMs,
                                    media: media, settlement: value.settlement.appValue,
                                    delivery: TeraPublicationEvidence.decode(value.delivery),
                                    targetDetails: TeraPublicationTargets.decode(value.targetDetails))
  }

  static func page(_ value: FfiSubmissionPageRecord, scope: TeraComposerScope, limit: UInt16) throws -> TeraSubmissionPage {
    try version(value.schemaVersion)
    guard try value.scope.composerAppValue == scope, value.entries.count <= Int(limit),
          limit > 0, limit <= 256, value.nextCursor?.isEmpty != true else { throw mismatch() }
    let entries: [TeraSubmissionEntry] = try value.entries.map { entry in
      switch entry {
      case let .submission(wire, reservationID, reservedAt, state):
        let request = try request(wire)
        guard request.scope == scope, validID(reservationID), reservedAt > 0,
              reservedAt <= UInt64(Int64.max) else { throw mismatch() }
        let summaryState = try summary(state)
        return .submission(TeraSubmissionSummary(request: request, reservationID: reservationID,
                                                 reservedAtUnixMilliseconds: reservedAt, state: summaryState))
      case let .repair(key, revision, reason):
        // A repair key is a locator, not authority for an editing or mutation API.
        guard key.utf8.count == 32, revision > 0, revision <= UInt64(Int64.max) else { throw mismatch() }
        let repair: TeraSubmissionRepairReason = switch reason {
        case .unsupportedSchema: .unsupportedSchema
        case .corruptRecord: .corruptRecord
        case .needsAttention: .needsAttention
        }
        return .repair(key: key, revision: revision, reason: repair)
      }
    }
    guard Set(entries.map(\.id)).count == entries.count else { throw mismatch() }
    return TeraSubmissionPage(scope: scope, entries: entries, nextCursor: value.nextCursor)
  }

  private static func summary(_ state: FfiSubmissionSummaryState) throws -> TeraSubmissionSummaryState {
    switch state {
    case .reserved: return .reserved
    case let .operation(intentID, operationID, revision, state):
      guard validID(intentID), validID(operationID), revision > 0,
            revision <= UInt64(Int64.max) else { throw mismatch() }
      return .operation(intentID: intentID, operationID: operationID, revision: revision, state: state.appValue)
    }
  }

  static func upload(_ value: FfiSubmissionUploadJobRecord, input: TeraSubmissionMediaRequest) throws -> TeraSubmissionUploadJob {
    guard value.schemaVersion == 2, !value.uploadUrl.isEmpty else { throw mismatch() }
    let status = try operation(value.submission, expected: input.request)
    guard input.expectedRevision < UInt64(Int64.max), status.revision == input.expectedRevision + 1,
          validID(value.operationId), value.expectedSha256 == input.media.media.sha256,
          value.mediaType == input.media.media.mediaType, value.byteSize == input.media.media.byteSize,
          value.authorizationHeader.hasPrefix("Nostr "),
          status.media.contains(where: { $0.opaqueReference == input.media.media.opaqueReference
              && $0.progress.url == value.remoteUrl && $0.progress.uploadURL == value.uploadUrl
              && $0.progress.stage == .uploading
          }) else { throw mismatch() }
    return TeraSubmissionUploadJob(submission: status, transfer: TeraNativeTransferJob(
      ownerID: status.intentID, expectedRevision: status.revision, operationID: value.operationId,
      remoteURL: value.remoteUrl, uploadURL: value.uploadUrl, authorizationHeader: value.authorizationHeader,
      expectedSHA256: value.expectedSha256, mediaType: value.mediaType, byteSize: value.byteSize
    ))
  }
}

extension FfiDraftMediaRecord {
  var appValue: TeraDraftMediaStatus {
    TeraDraftMediaStatus(
      url: url,
      stage: stage.appValue,
      uploadAttempts: uploadAttempts,
      verifiedAtUnixMilliseconds: verifiedAtUnixMs,
      possibleOrphan: possibleOrphan,
      orphanReasonCode: orphanReasonCode,
      orphanRecordedAtUnixMilliseconds: orphanRecordedAtUnixMs,
      uploadURL: schemaVersion == 2 ? uploadUrl : nil
    )
  }
}

extension FfiNativeUploadJobRecord {
  var appValue: TeraNativeUploadJob {
    get throws {
      guard schemaVersion == 2, !uploadUrl.isEmpty else { throw TeraGeneratedSubmission.mismatch() }
      return TeraNativeUploadJob(
        operationID: operationId,
        draft: draft.appValue,
        remoteURL: remoteUrl,
        uploadURL: uploadUrl,
        authorizationHeader: authorizationHeader,
        expectedSHA256: expectedSha256,
        mediaType: mediaType,
        byteSize: byteSize
      )
    }
  }
}
