import Foundation

protocol TeraRuntimeSubscriptionToken: Sendable {
  func cancel() async
}

protocol TeraRuntimeBackend: Sendable {
  func snapshot() async throws -> TeraRuntimeSnapshot
  func todayPage(request: TeraTodayPageRequest) async throws -> TeraTodayPage
  func refreshToday(
    context: TeraLocalNetwork,
    nowUnixSeconds: UInt64,
    update: TeraTodayProjectionUpdate,
    backfillCursor: String?
  ) async throws -> TeraTodaySyncReceipt
  func search(
    context: TeraLocalNetwork,
    query: String,
    limit: UInt16,
    asOfUnixSeconds: UInt64
  ) async throws -> [TeraSearchResult]
  func me(
    context: TeraLocalNetwork,
    asOfUnixSeconds: UInt64
  ) async throws -> TeraMeSnapshot
  func retrieveMedia(
    context: TeraLocalNetwork,
    reference: TeraMediaReference
  ) async throws -> TeraVerifiedMediaArtifact
  func verifiedMediaArtifact(
    context: TeraLocalNetwork,
    artifactID: String
  ) async throws -> TeraVerifiedMediaArtifact?
  func invalidateMediaArtifact(
    context: TeraLocalNetwork,
    artifactID: String
  ) async throws -> Bool
  func addSchemas() async throws -> [TeraAddSchema]
  func saveAddIntent(
    input: TeraAddRuntimeInput,
    existingDraftID: String?,
    expectedRevision: UInt64?
  ) async throws -> TeraDraftStatus
  func saveRetractionDraft(
    id: String,
    input: TeraRetractionDraftInput,
    authoredAtUnixSeconds: UInt64,
    persistedAtUnixMilliseconds: UInt64
  ) async throws -> TeraDraftStatus
  func saveRevisionIntent(
    target: TeraRevisionTarget,
    replacement: TeraAddRuntimeInput
  ) async throws -> TeraRevisionStatus
  func revisionStatus(operationID: String) async throws -> TeraRevisionStatus
  func advanceRevision(operationID: String) async throws -> TeraRevisionStatus
  func cancelRevision(operationID: String) async throws -> TeraRevisionStatus
  func draftStatus(id: String) async throws -> TeraDraftStatus
  func draftHeads(limit: UInt16) async throws -> [TeraDraftStatus]
  func queueAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> TeraDraftStatus
  func recoverAddIntent(id: String) async throws -> TeraDraftStatus
  func uploadAddMediaIntent(input: TeraBlossomUploadIntent) async throws -> TeraDraftStatus
  func prepareAddMediaBackground(
    input: TeraBlossomUploadIntent
  ) async throws -> TeraNativeUploadJob
  func completeAddMediaBackground(
    input: TeraNativeUploadCompletion
  ) async throws -> TeraDraftStatus
  func probeBlossom() async throws -> TeraBlossomEvidence
  func mobileSettings() async throws -> TeraMobileSettings
  func replaceMobileSettings(
    input: TeraReplaceSettings
  ) async throws -> TeraSettingsTransition
  func applyIdentityCommand(
    expectedRevision: UInt64,
    command: TeraIdentityCommand
  ) async throws -> TeraSettingsTransition
  func saveProfileMetadata(input: TeraProfileMetadataInput) async throws
    -> TeraProfileStatus
  func profileStatus(operationID: String) async throws -> TeraProfileStatus
  func advanceProfile(operationID: String) async throws -> TeraProfileStatus
  func cancelProfile(operationID: String, expectedRevision: UInt64) async throws
    -> TeraProfileStatus
  func advanceDraft(id: String, expectedRevision: UInt64) async throws -> TeraDraftStatus
  func cancelAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> TeraDraftStatus
  func subscribe(
    bufferCapacity: Int,
    receive: @escaping @Sendable (TeraRuntimeChange) async -> Void
  ) async throws -> any TeraRuntimeSubscriptionToken
  func shutdown() async throws -> TeraRuntimeShutdownReceipt
}

extension TeraRuntimeBackend {
  private func supportUnsupported() -> TeraRuntimeFailure {
    .local(
      operation: "runtime.support",
      code: "ios.support.unsupported",
      safeMessage: "This supporting surface is unavailable in the current runtime."
    )
  }

  private func addUnsupported() -> TeraRuntimeFailure {
    .local(
      operation: "runtime.add",
      code: "ios.add.unsupported",
      safeMessage: "Add is unavailable in this runtime."
    )
  }

  func addSchemas() async throws -> [TeraAddSchema] {
    throw addUnsupported()
  }

  func saveAddIntent(
    input _: TeraAddRuntimeInput,
    existingDraftID _: String?,
    expectedRevision _: UInt64?
  ) async throws -> TeraDraftStatus {
    throw addUnsupported()
  }

  func saveRetractionDraft(
    id _: String,
    input _: TeraRetractionDraftInput,
    authoredAtUnixSeconds _: UInt64,
    persistedAtUnixMilliseconds _: UInt64
  ) async throws -> TeraDraftStatus {
    throw addUnsupported()
  }

  func saveRevisionIntent(
    target _: TeraRevisionTarget,
    replacement _: TeraAddRuntimeInput
  ) async throws -> TeraRevisionStatus {
    throw addUnsupported()
  }

  func revisionStatus(operationID _: String) async throws -> TeraRevisionStatus {
    throw addUnsupported()
  }

  func advanceRevision(operationID _: String) async throws -> TeraRevisionStatus {
    throw addUnsupported()
  }

  func cancelRevision(operationID _: String) async throws -> TeraRevisionStatus {
    throw addUnsupported()
  }

  func draftStatus(id _: String) async throws -> TeraDraftStatus {
    throw addUnsupported()
  }

  func draftHeads(limit _: UInt16) async throws -> [TeraDraftStatus] {
    throw addUnsupported()
  }

  func queueAddIntent(
    id _: String,
    expectedRevision _: UInt64
  ) async throws -> TeraDraftStatus {
    throw addUnsupported()
  }

  func recoverAddIntent(id _: String) async throws -> TeraDraftStatus {
    throw addUnsupported()
  }

  func uploadAddMediaIntent(input _: TeraBlossomUploadIntent) async throws
    -> TeraDraftStatus
  {
    throw addUnsupported()
  }

  func prepareAddMediaBackground(
    input _: TeraBlossomUploadIntent
  ) async throws -> TeraNativeUploadJob {
    throw addUnsupported()
  }

  func completeAddMediaBackground(
    input _: TeraNativeUploadCompletion
  ) async throws -> TeraDraftStatus {
    throw addUnsupported()
  }

  func probeBlossom() async throws -> TeraBlossomEvidence {
    throw supportUnsupported()
  }

  func mobileSettings() async throws -> TeraMobileSettings {
    throw supportUnsupported()
  }

  func replaceMobileSettings(
    input _: TeraReplaceSettings
  ) async throws -> TeraSettingsTransition {
    throw supportUnsupported()
  }

  func applyIdentityCommand(
    expectedRevision _: UInt64,
    command _: TeraIdentityCommand
  ) async throws -> TeraSettingsTransition {
    throw supportUnsupported()
  }

  func saveProfileMetadata(input _: TeraProfileMetadataInput) async throws
    -> TeraProfileStatus
  {
    throw supportUnsupported()
  }

  func profileStatus(operationID _: String) async throws -> TeraProfileStatus {
    throw supportUnsupported()
  }

  func advanceProfile(operationID _: String) async throws -> TeraProfileStatus {
    throw supportUnsupported()
  }

  func cancelProfile(operationID _: String, expectedRevision _: UInt64) async throws
    -> TeraProfileStatus
  {
    throw supportUnsupported()
  }

  func advanceDraft(id _: String, expectedRevision _: UInt64) async throws -> TeraDraftStatus {
    throw addUnsupported()
  }

  func cancelAddIntent(
    id _: String,
    expectedRevision _: UInt64
  ) async throws -> TeraDraftStatus {
    throw addUnsupported()
  }

  func search(
    context _: TeraLocalNetwork,
    query _: String,
    limit _: UInt16,
    asOfUnixSeconds _: UInt64
  ) async throws -> [TeraSearchResult] {
    throw supportUnsupported()
  }

  func me(
    context _: TeraLocalNetwork,
    asOfUnixSeconds _: UInt64
  ) async throws -> TeraMeSnapshot {
    throw supportUnsupported()
  }

  func retrieveMedia(
    context _: TeraLocalNetwork,
    reference _: TeraMediaReference
  ) async throws -> TeraVerifiedMediaArtifact {
    throw supportUnsupported()
  }

  func verifiedMediaArtifact(
    context _: TeraLocalNetwork,
    artifactID _: String
  ) async throws -> TeraVerifiedMediaArtifact? {
    throw supportUnsupported()
  }

  func invalidateMediaArtifact(
    context _: TeraLocalNetwork,
    artifactID _: String
  ) async throws -> Bool {
    throw supportUnsupported()
  }
}

struct TeraRuntimeBackendStart: Sendable {
  let backend: any TeraRuntimeBackend
  let snapshot: TeraRuntimeSnapshot
}

typealias TeraRuntimeBackendFactory =
  @Sendable (
    TeraRuntimeLaunchConfiguration
  ) async throws -> TeraRuntimeBackendStart
