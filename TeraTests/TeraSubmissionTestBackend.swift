import Foundation
@testable import TeraApp

/// Controllable native-boundary test double. Production policy is exercised by
/// the separate generated FFI/SQLite tests, never inferred from this scheduler.
actor SubmissionTestBackend {
  let composer: ComposerTestStorage
  let writable: Bool
  let offline: Bool
  let delayedPhase: AddDelayPhase?
  let delayAfterCompletion: Bool
  private(set) var idCount = 0
  private(set) var prepareCount = 0
  private(set) var advanceCount = 0
  private(set) var uploadCount = 0
  private(set) var completionPersisted = false
  private var reservations: [String: TeraSubmissionReservation] = [:]
  private var operations: [String: TeraSubmissionStatus] = [:]
  var operationCount: Int {
    operations.count
  }

  private var preparePause: ResourceTestPause?
  private var advancePause: ResourceTestPause?
  private var failAfterCommit = false
  private var failReads = false
  private var delayed = false

  init(composer: ComposerTestStorage, writable: Bool, offline: Bool, delayedPhase: AddDelayPhase?, delayAfterCompletion: Bool) {
    self.composer = composer
    self.writable = writable
    self.offline = offline
    self.delayedPhase = delayedPhase
    self.delayAfterCompletion = delayAfterCompletion
  }

  func pausePrepare(_ pause: ResourceTestPause, loseReceipt: Bool = false) {
    preparePause = pause
    failAfterCommit = loseReceipt
  }

  func pauseAdvance(_ pause: ResourceTestPause) {
    advancePause = pause
  }

  func unreadable(_ value: Bool) {
    failReads = value
  }

  func reserveID() -> String {
    idCount += 1
    return String(format: "%032x", 1000 + idCount)
  }

  func reserve(_ request: TeraSubmissionRequest) async throws -> TeraSubmissionReservation {
    if let existing = reservations[request.commandID] {
      guard existing.captured.scope == request.scope, existing.captured.id == request.composerID,
            existing.captured.revision == request.expectedRevision else { throw failure("idempotency_conflict") }
      return existing
    }
    let source = try await composer.load(request.scope, id: request.composerID)
    guard source.revision == request.expectedRevision else { throw failure("composer_revision_conflict") }
    let value = TeraSubmissionReservation(commandID: request.commandID, reservationID: request.commandID,
                                          captured: source, reservedAtUnixMilliseconds: 1_800_000_000_000, replayed: false)
    reservations[request.commandID] = value
    return value
  }

  func recover(_ request: TeraSubmissionRequest) throws -> TeraSubmissionStatus? {
    if failReads {
      throw failure("storage_unavailable")
    }
    guard let value = operations[request.commandID] else { return nil }
    guard value.request == request else { throw failure("idempotency_conflict") }
    return value
  }

  func status(_ request: TeraSubmissionRequest) throws -> TeraSubmissionStatus {
    guard let value = try recover(request) else { throw failure("submission_not_found") }
    return value
  }

  func prepare(_ request: TeraSubmissionRequest, media: [TeraPreparedMediaHandle]) async throws -> TeraSubmissionStatus {
    if let value = try recover(request) {
      return value
    }
    let reservation = try await reserve(request)
    guard writable else { throw failure("submission_policy_unavailable") }
    guard media.map(\.media.opaqueReference) == reservation.captured.form.media.map(\.opaqueReference) else {
      throw failure("submission_media_invalid")
    }
    prepareCount += 1
    let initial = TeraSubmissionStatus(
      request: request, intentID: String(format: "%032x", 2000 + prepareCount),
      operationID: String(format: "%032x", 3000 + prepareCount), revision: 1, captured: reservation.captured,
      state: media.isEmpty ? .readyToSign : .mediaPreparing,
      committedAtUnixMilliseconds: 1_800_000_000_000, updatedAtUnixMilliseconds: 1_800_000_000_000,
      media: media.map { TeraSubmissionMedia(opaqueReference: $0.media.opaqueReference,
                                             progress: progress($0.media, stage: .pending)) }, settlement: settlement(complete: false)
    )
    if delayedPhase == .queue, !delayed {
      delayed = true
      try await Task.sleep(for: .milliseconds(50))
    }
    operations[request.commandID] = initial
    if let pause = preparePause {
      preparePause = nil; await pause.wait()
    }
    if failAfterCommit {
      failAfterCommit = false; throw failure("storage_unavailable")
    }
    return initial
  }

  func advance(_ request: TeraSubmissionRequest, revision: UInt64) async throws -> TeraSubmissionStatus {
    let value = try status(request)
    guard value.revision == revision else { throw failure("draft_revision_conflict") }
    advanceCount += 1
    let queued = replacing(value, state: .queued)
    operations[request.commandID] = queued
    if let pause = advancePause {
      advancePause = nil; await pause.wait()
    }
    if delayedPhase == .advance, !delayed {
      delayed = true
      try await Task.sleep(for: .milliseconds(50))
    }
    if offline {
      throw failure("relay_offline")
    }
    let complete = replacing(queued, state: .complete)
    operations[request.commandID] = complete
    return complete
  }

  func upload(_ input: TeraSubmissionMediaRequest) throws -> TeraSubmissionUploadJob {
    let value = try status(input.request)
    guard value.revision == input.expectedRevision else { throw failure("draft_revision_conflict") }
    uploadCount += 1
    let current = replacing(value, state: .mediaUploading, media: value.media.map {
      $0.opaqueReference == input.media.media.opaqueReference
        ? TeraSubmissionMedia(opaqueReference: $0.opaqueReference, progress: progress(input.media.media, stage: .uploading)) : $0
    })
    operations[input.request.commandID] = current
    return TeraSubmissionUploadJob(submission: current, transfer: TeraNativeTransferJob(
      ownerID: current.intentID, expectedRevision: current.revision,
      operationID: String(format: "%032x", 4000 + uploadCount),
      remoteURL: "http://127.0.0.1:3000/\(input.media.media.sha256).png", uploadURL: "http://127.0.0.1:3000/upload",
      authorizationHeader: "Nostr test-token",
      expectedSHA256: input.media.media.sha256, mediaType: input.media.media.mediaType, byteSize: input.media.media.byteSize
    ))
  }

  func complete(_ input: TeraSubmissionMediaRequest, response: TeraAddBackgroundUploadReceipt) async throws -> TeraSubmissionStatus {
    let value = try status(input.request)
    guard value.revision == input.expectedRevision, response.expectedRevision == value.revision,
          response.draftID == value.intentID else { throw failure("submission_media_invalid") }
    let current = replacing(value, state: .readyToSign, media: value.media.map {
      $0.opaqueReference == input.media.media.opaqueReference
        ? TeraSubmissionMedia(opaqueReference: $0.opaqueReference, progress: progress(input.media.media, stage: .verified)) : $0
    })
    operations[input.request.commandID] = current
    completionPersisted = true
    if delayAfterCompletion {
      try await Task.sleep(for: .milliseconds(50))
    }
    return current
  }

  func page(scope: TeraComposerScope, limit: UInt16, cursor: String?) throws -> TeraSubmissionPage {
    if failReads {
      throw failure("storage_unavailable")
    }
    let matches = reservations.values.filter { $0.captured.scope == scope && $0.commandID > (cursor ?? "") }
      .sorted { $0.commandID < $1.commandID }
    let entries = matches.prefix(Int(limit)).map { value in
      let request = TeraSubmissionRequest(commandID: value.commandID, scope: scope,
                                          composerID: value.captured.id, expectedRevision: value.captured.revision)
      let state = operations[value.commandID].map {
        TeraSubmissionSummaryState.operation(intentID: $0.intentID, operationID: $0.operationID, revision: $0.revision, state: $0.state)
      } ?? .reserved
      return TeraSubmissionEntry.submission(TeraSubmissionSummary(request: request, reservationID: value.reservationID,
                                                                  reservedAtUnixMilliseconds: value.reservedAtUnixMilliseconds, state: state))
    }
    return TeraSubmissionPage(scope: scope, entries: entries,
                              nextCursor: matches.count > Int(limit) ? entries.last?.id : nil)
  }

  private func replacing(_ value: TeraSubmissionStatus, state: TeraOutboxState, media: [TeraSubmissionMedia]? = nil) -> TeraSubmissionStatus {
    TeraSubmissionStatus(request: value.request, intentID: value.intentID, operationID: value.operationID,
                         revision: value.revision + 1, captured: value.captured, state: state,
                         committedAtUnixMilliseconds: value.committedAtUnixMilliseconds,
                         updatedAtUnixMilliseconds: value.updatedAtUnixMilliseconds + 1, media: media ?? value.media,
                         settlement: settlement(complete: state == .complete))
  }

  private func progress(_ media: TeraPreparedMedia, stage: TeraDraftMediaStage) -> TeraDraftMediaStatus {
    TeraDraftMediaStatus(url: "http://127.0.0.1:3000/\(media.sha256).png", stage: stage, uploadAttempts: stage == .pending ? 0 : 1,
                         verifiedAtUnixMilliseconds: stage == .verified ? 1_800_000_000_000 : nil,
                         possibleOrphan: false, orphanReasonCode: nil, orphanRecordedAtUnixMilliseconds: nil)
  }

  private func settlement(complete: Bool) -> TeraOperationSettlement {
    TeraOperationSettlement(artifacts: 1, signed: complete ? 1 : 0, admitted: complete ? 1 : 0,
                            pending: complete ? 0 : 1, retryable: 0, indeterminate: 0, failedTerminal: 0, cancelled: 0,
                            deliveryPlans: 1, deliverySatisfied: complete ? 1 : 0, deliveryPending: complete ? 0 : 1,
                            deliveryRetryable: 0, deliveryExhausted: 0, deliveryFailedTerminal: 0, deliveryCancelled: 0)
  }

  private func failure(_ code: String) -> TeraRuntimeFailure {
    .local(operation: "test.submission", code: code, safeMessage: "Submission needs attention.")
  }
}

extension AddBackend {
  func reserveSubmissionID() async -> String {
    await submissionBackend.reserveID()
  }

  func reserveSubmission(request: TeraSubmissionRequest) async throws -> TeraSubmissionReservation {
    try await submissionBackend.reserve(request)
  }

  func prepareSubmission(request: TeraSubmissionRequest, media: [TeraPreparedMediaHandle]) async throws -> TeraSubmissionStatus {
    try await submissionBackend.prepare(request, media: media)
  }

  func recoverSubmission(request: TeraSubmissionRequest) async throws -> TeraSubmissionStatus? {
    try await submissionBackend.recover(request)
  }

  func submissionStatus(request: TeraSubmissionRequest) async throws -> TeraSubmissionStatus {
    try await submissionBackend.status(request)
  }

  func advanceSubmission(request: TeraSubmissionRequest, expectedRevision: UInt64) async throws -> TeraSubmissionStatus {
    try await submissionBackend.advance(request, revision: expectedRevision)
  }

  func listSubmissions(scope: TeraComposerScope, limit: UInt16, cursor: String?) async throws -> TeraSubmissionPage {
    try await submissionBackend.page(scope: scope, limit: limit, cursor: cursor)
  }

  func prepareSubmissionUpload(input: TeraSubmissionMediaRequest) async throws -> TeraSubmissionUploadJob {
    try await submissionBackend.upload(input)
  }

  func completeSubmissionUpload(input: TeraSubmissionMediaRequest, response: TeraAddBackgroundUploadReceipt) async throws -> TeraSubmissionStatus {
    try await submissionBackend.complete(input, response: response)
  }
}
