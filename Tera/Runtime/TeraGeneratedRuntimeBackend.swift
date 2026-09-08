import Foundation
import TeraKitBindings

final class TeraGeneratedHostSigner: TeraHostSigner, @unchecked Sendable {
  typealias QualificationEvidenceRecorder = @Sendable (HostSigningRequest, String) throws -> Void

  private let signer: any TeraRuntimeSigner
  private let clock: TeraClock
  private let qualificationEvidenceRecorder: QualificationEvidenceRecorder

  init(
    signer: any TeraRuntimeSigner,
    clock: TeraClock = .system,
    qualificationEvidenceRecorder: @escaping QualificationEvidenceRecorder =
      TeraRemoteQualificationEvidence.recordBlossomAuthorization
  ) {
    self.signer = signer
    self.clock = clock
    self.qualificationEvidenceRecorder = qualificationEvidenceRecorder
  }

  func signerStatus() async -> SignerStatusRecord {
    await SignerStatusRecord(
      schemaVersion: 1,
      availability: signer.availability().generatedValue
    )
  }

  func sign(request: HostSigningRequest) async -> HostSigningResult {
    guard (try? clock.unixMilliseconds()) != nil else {
      return failedSigningResult(for: request)
    }
    let purpose = request.purpose.appValue
    let outcome = await signer.sign(
      TeraRuntimeSigningRequest(
        operationID: request.operationId,
        signerRequestID: request.signerRequestId,
        publicKeyHex: request.publicKey,
        purpose: purpose,
        deadlineUnixMilliseconds: request.deadlineUnixMs,
        digest: request.eventIdDigest
      )
    )
    #if DEBUG
      if request.purpose == .blossomUpload,
         let signatureHex = outcome.signatureHex
      {
        do {
          try qualificationEvidenceRecorder(request, signatureHex)
        } catch {
          return failedSigningResult(for: request)
        }
      }
    #endif
    guard let completedAtUnixMilliseconds = try? clock.unixMilliseconds() else {
      return failedSigningResult(for: request)
    }
    return HostSigningResult(
      schemaVersion: 1,
      outcome: outcome.generatedOutcome,
      operationId: request.operationId,
      signerRequestId: request.signerRequestId,
      publicKey: request.publicKey,
      purpose: request.purpose,
      signatureHex: outcome.signatureHex,
      completedAtUnixMs: completedAtUnixMilliseconds
    )
  }

  private func failedSigningResult(for request: HostSigningRequest) -> HostSigningResult {
    HostSigningResult(
      schemaVersion: 1,
      outcome: .failed,
      operationId: request.operationId,
      signerRequestId: request.signerRequestId,
      publicKey: request.publicKey,
      purpose: request.purpose,
      signatureHex: nil,
      completedAtUnixMs: 0
    )
  }
}

private final class TeraGeneratedRuntimeObserver: TeraRuntimeObserver, @unchecked Sendable {
  private let continuation: AsyncStream<TeraRuntimeChange>.Continuation

  init(continuation: AsyncStream<TeraRuntimeChange>.Continuation) {
    self.continuation = continuation
  }

  func onChange(change: FfiRuntimeChangeRecord) {
    continuation.yield(
      TeraRuntimeChange(
        schemaVersion: change.schemaVersion,
        generation: change.generation,
        kind: change.kind.appValue,
        entityID: change.entityId
      )
    )
  }

  func finish() {
    continuation.finish()
  }
}

private actor TeraGeneratedSubscriptionToken: TeraRuntimeSubscriptionToken {
  private var handle: FfiSubscriptionHandle?
  private var observer: TeraGeneratedRuntimeObserver?
  private var deliveryTask: Task<Void, Never>?

  init(
    handle: FfiSubscriptionHandle,
    observer: TeraGeneratedRuntimeObserver,
    deliveryTask: Task<Void, Never>
  ) {
    self.handle = handle
    self.observer = observer
    self.deliveryTask = deliveryTask
  }

  func cancel() async {
    guard let handle else { return }
    self.handle = nil
    observer?.finish()
    observer = nil
    handle.unsubscribe()
    deliveryTask?.cancel()
    await deliveryTask?.value
    deliveryTask = nil
  }
}

private final class TeraGeneratedRuntimeBackend: TeraRuntimeBackend, @unchecked Sendable {
  private let runtime: TeraRuntime

  init(runtime: TeraRuntime) {
    self.runtime = runtime
  }

  func snapshot() async throws -> TeraRuntimeSnapshot {
    do {
      let identity = try runtime.identityStatus()
      let relay = try runtime.sdkRelayStatus()
      let blossomConfiguration = try runtime.sdkBlossomConfiguration()
      let blossomEvidence = try runtime.sdkBlossomEvidence()
      let info = runtime.info()
      return TeraRuntimeSnapshot(
        identity: TeraRuntimeIdentity(
          publicKeyHex: identity.publicKey,
          hostSignerConfigured: identity.hostSignerConfigured
        ),
        relay: relay.map { report in
          TeraRelayStatus(
            profile: report.profile,
            state: report.state,
            readAvailability: report.readAvailability,
            writeAvailability: report.writeAvailability,
            relays: report.relays.map { relay in
              TeraRelayEndpointStatus(
                url: relay.relayUrl,
                access: relay.access.appValue,
                readState: relay.readState,
                writeState: relay.writeState,
                readLastAttemptUnixMilliseconds: relay.readLastAttemptUnixMs,
                writeLastAttemptUnixMilliseconds: relay.writeLastAttemptUnixMs,
                readNextAttemptUnixMilliseconds: relay.readNextAttemptUnixMs,
                writeNextAttemptUnixMilliseconds: relay.writeNextAttemptUnixMs
              )
            }
          )
        },
        blossomConfiguration: blossomConfiguration?.appValue,
        blossomEvidence: blossomEvidence?.appValue,
        crateName: info.sdk.crateName,
        crateVersion: info.sdk.crateVersion,
        isClosed: info.sdkClosed
      )
    } catch {
      throw Self.failure(from: error)
    }
  }

  func todayPage(request: TeraTodayPageRequest) async throws -> TeraTodayPage {
    do {
      let page = try await runtime.phase1TodayPage(
        context: request.context.generatedValue,
        limit: request.limit,
        asOfUnixS: request.asOfUnixSeconds,
        cursor: request.cursor
      )
      return page.appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func refreshToday(
    context: TeraLocalNetwork,
    nowUnixSeconds: UInt64,
    update: TeraTodayProjectionUpdate
  ) async throws -> TeraTodayRefreshReceipt {
    do {
      let receipt = try await runtime.phase1SyncToday(
        context: context.generatedValue,
        nowUnixS: nowUnixSeconds,
        update: update.generatedValue
      )
      switch receipt.relayState {
      case .complete:
        return receipt.projection.appValue
      case .partial:
        throw TeraRuntimeFailure(
          schemaVersion: 1,
          code: "today_relay_partial",
          category: "relay",
          retryable: true,
          recoveryActions: ["retry"],
          operationID: "runtime.today.refresh",
          capabilityID: "nostr_source",
          safeMessage: "Today refreshed from only part of the local network."
        )
      case .offline:
        throw TeraRuntimeFailure(
          schemaVersion: 1,
          code: "today_relay_offline",
          category: "relay",
          retryable: true,
          recoveryActions: ["retry"],
          operationID: "runtime.today.refresh",
          capabilityID: "nostr_source",
          safeMessage: "Today is showing saved posts because the local network is offline."
        )
      }
    } catch {
      throw Self.failure(from: error)
    }
  }

  func search(
    context: TeraLocalNetwork,
    query: String,
    limit: UInt16,
    asOfUnixSeconds: UInt64
  ) async throws -> [TeraSearchResult] {
    do {
      return try await runtime.phase1Search(
        context: context.generatedValue,
        query: query,
        limit: limit,
        asOfUnixS: asOfUnixSeconds
      ).map(\.appValue)
    } catch {
      throw Self.failure(from: error)
    }
  }

  func me(
    context: TeraLocalNetwork,
    asOfUnixSeconds: UInt64
  ) async throws -> TeraMeSnapshot {
    do {
      return try await runtime.phase1Me(
        context: context.generatedValue,
        asOfUnixS: asOfUnixSeconds
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func retrieveMedia(
    context: TeraLocalNetwork,
    reference: TeraMediaReference
  ) async throws -> TeraVerifiedMediaArtifact {
    do {
      let operation = try FfiMediaOperation()
      let record = try await withTaskCancellationHandler {
        try await runtime.phase1RetrieveMedia(
          context: context.generatedValue,
          referenceFingerprint: reference.referenceFingerprint,
          operation: operation
        )
      } onCancel: {
        operation.cancel()
      }
      return try Self.verifiedMediaArtifact(
        record,
        expectedArtifactID: reference.sha256,
        requiresOperationID: true
      )
    } catch {
      throw Self.failure(from: error)
    }
  }

  func verifiedMediaArtifact(
    context: TeraLocalNetwork,
    artifactID: String
  ) async throws -> TeraVerifiedMediaArtifact? {
    do {
      guard let record = try await runtime.phase1VerifiedMediaArtifact(
        context: context.generatedValue,
        artifactId: artifactID
      ) else { return nil }
      return try Self.verifiedMediaArtifact(
        record,
        expectedArtifactID: artifactID,
        requiresOperationID: false
      )
    } catch {
      throw Self.failure(from: error)
    }
  }

  func invalidateMediaArtifact(
    context: TeraLocalNetwork,
    artifactID: String
  ) async throws -> Bool {
    do {
      return try await runtime.phase1InvalidateMediaArtifact(
        context: context.generatedValue,
        artifactId: artifactID
      )
    } catch {
      throw Self.failure(from: error)
    }
  }

  func addSchemas() async throws -> [TeraAddSchema] {
    runtime.phase1AddSchemas().map(\.appValue)
  }

  func saveAddIntent(
    input: TeraAddRuntimeInput,
    existingDraftID: String?,
    expectedRevision: UInt64?
  ) async throws -> TeraDraftStatus {
    do {
      return try await runtime.phase1SaveAddIntent(
        input: input.generatedValue,
        existingDraftId: existingDraftID,
        expectedRevision: expectedRevision
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func saveRetractionDraft(
    id: String,
    input: TeraRetractionDraftInput,
    authoredAtUnixSeconds: UInt64,
    persistedAtUnixMilliseconds: UInt64
  ) async throws -> TeraDraftStatus {
    do {
      return try await runtime.phase1SaveRetractionDraft(
        draftId: id,
        input: input.generatedValue,
        authoredAtUnixS: authoredAtUnixSeconds,
        persistedAtUnixMs: persistedAtUnixMilliseconds
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func saveRevisionIntent(
    target: TeraRevisionTarget,
    replacement: TeraAddRuntimeInput
  ) async throws -> TeraRevisionStatus {
    do {
      return try await runtime.phase1SaveRevisionIntent(
        input: FfiRevisionInputRecord(
          schemaVersion: 1,
          cardId: target.cardID,
          sourceEventId: target.sourceEventID,
          sourceAddress: target.sourceAddress,
          authorPublicKey: target.authorPublicKey,
          replacement: replacement.generatedValue
        )
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func revisionStatus(operationID: String) async throws -> TeraRevisionStatus {
    do {
      return try await runtime.phase1RevisionStatus(operationId: operationID).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func advanceRevision(operationID: String) async throws -> TeraRevisionStatus {
    do {
      return try await runtime.phase1AdvanceRevision(operationId: operationID).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func cancelRevision(operationID: String) async throws -> TeraRevisionStatus {
    do {
      return try await runtime.phase1CancelRevision(operationId: operationID).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func draftStatus(id: String) async throws -> TeraDraftStatus {
    do {
      return try await runtime.phase1DraftStatus(draftId: id).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func draftHeads(limit: UInt16) async throws -> [TeraDraftStatus] {
    do {
      return try await runtime.phase1DraftHeads(limit: limit).map(\.appValue)
    } catch {
      throw Self.failure(from: error)
    }
  }

  func queueAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> TeraDraftStatus {
    do {
      return try await runtime.phase1QueueAddIntent(
        draftId: id,
        expectedRevision: expectedRevision
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func recoverAddIntent(id: String) async throws -> TeraDraftStatus {
    do {
      return try await runtime.phase1RecoverAddIntent(draftId: id).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func uploadAddMediaIntent(input: TeraBlossomUploadIntent) async throws -> TeraDraftStatus {
    do {
      return try await runtime.phase1UploadAddMediaIntent(input: input.generatedValue).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func prepareAddMediaBackground(
    input: TeraBlossomUploadIntent
  ) async throws -> TeraNativeUploadJob {
    do {
      return try await runtime.phase1PrepareAddMediaBackground(
        input: input.generatedValue
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func completeAddMediaBackground(
    input: TeraNativeUploadCompletion
  ) async throws -> TeraDraftStatus {
    do {
      return try await runtime.phase1CompleteAddMediaBackground(
        input: input.generatedValue
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func probeBlossom() async throws -> TeraBlossomEvidence {
    do {
      return try await runtime.probeBlossom().appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func mobileSettings() async throws -> TeraMobileSettings {
    do {
      return try await runtime.phase1Settings().appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func replaceMobileSettings(
    input: TeraReplaceSettings
  ) async throws -> TeraSettingsTransition {
    do {
      return try await runtime.phase1ReplaceSettings(input: input.generatedValue).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func applyIdentityCommand(
    expectedRevision: UInt64,
    command: TeraIdentityCommand
  ) async throws -> TeraSettingsTransition {
    do {
      return try await runtime.phase1ApplyIdentityCommand(
        expectedRevision: expectedRevision,
        command: command.generatedValue
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func saveProfileMetadata(input: TeraProfileMetadataInput) async throws
    -> TeraProfileStatus
  {
    do {
      return try await runtime.phase1SaveProfileMetadata(input: input.generatedValue).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func profileStatus(operationID: String) async throws -> TeraProfileStatus {
    do {
      return try await runtime.phase1ProfileStatus(operationId: operationID).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func advanceProfile(operationID: String) async throws -> TeraProfileStatus {
    do {
      return try await runtime.phase1AdvanceProfile(operationId: operationID).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func cancelProfile(
    operationID: String,
    expectedRevision: UInt64
  ) async throws -> TeraProfileStatus {
    do {
      return try await runtime.phase1CancelProfile(
        operationId: operationID,
        expectedRevision: expectedRevision
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func advanceDraft(id: String, expectedRevision: UInt64) async throws -> TeraDraftStatus {
    do {
      return try await runtime.phase1AdvanceDraft(
        draftId: id,
        expectedRevision: expectedRevision
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func cancelAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> TeraDraftStatus {
    do {
      return try await runtime.phase1CancelAddIntent(
        draftId: id,
        expectedRevision: expectedRevision
      ).appValue
    } catch {
      throw Self.failure(from: error)
    }
  }

  func subscribe(
    bufferCapacity: Int,
    receive: @escaping @Sendable (TeraRuntimeChange) async -> Void
  ) async throws -> any TeraRuntimeSubscriptionToken {
    let pair = AsyncStream.makeStream(
      of: TeraRuntimeChange.self,
      bufferingPolicy: .bufferingNewest(bufferCapacity)
    )
    do {
      let observer = TeraGeneratedRuntimeObserver(continuation: pair.continuation)
      let handle = try runtime.subscribeChanges(observer: observer)
      let deliveryTask = Task {
        for await change in pair.stream {
          guard !Task.isCancelled else { break }
          await receive(change)
        }
      }
      return TeraGeneratedSubscriptionToken(
        handle: handle,
        observer: observer,
        deliveryTask: deliveryTask
      )
    } catch {
      pair.continuation.finish()
      throw Self.failure(from: error)
    }
  }

  func shutdown() async throws -> TeraRuntimeShutdownReceipt {
    do {
      let receipt = try await runtime.shutdown()
      return TeraRuntimeShutdownReceipt(
        state: receipt.state,
        alreadyClosed: receipt.alreadyClosed
      )
    } catch {
      throw Self.failure(from: error)
    }
  }

  static func start(
    configuration: TeraRuntimeLaunchConfiguration
  ) async throws -> TeraRuntimeBackendStart {
    var createdRuntime: TeraRuntime?
    do {
      let runtime = try await TeraRuntime.withHostSigner(
        applicationSupportDirectory: configuration.applicationSupportDirectory,
        publicKeyHex: configuration.publicKeyHex,
        sourceGenerationHex: configuration.sourceGenerationHex,
        sourceGenerationCreatedAtUnixMs: configuration.sourceGenerationCreatedAtUnixMilliseconds,
        protectedData: configuration.protectedData.generatedValue,
        hostSigner: TeraGeneratedHostSigner(signer: configuration.signer)
      )
      createdRuntime = runtime
      runtime.setAppInfoPlatform(
        platform: "iOS",
        bundleId: configuration.app.bundleIdentifier,
        version: configuration.app.version,
        buildNumber: configuration.app.buildNumber,
        buildSha: configuration.app.buildSHA
      )

      let backend = TeraGeneratedRuntimeBackend(runtime: runtime)
      let currentSettings = try await backend.mobileSettings()
      if configuration.adoptBootstrapSettings
        || currentSettings.revision == 1 && currentSettings.identity.identities.isEmpty
      {
        _ = try await backend.replaceMobileSettings(
          input: configuration.bootstrapSettingsReplacement(current: currentSettings)
        )
      }
      _ = try await runtime.phase1ApplySettingsToRuntime()
      return try await TeraRuntimeBackendStart(
        backend: backend,
        snapshot: backend.snapshot()
      )
    } catch {
      if let createdRuntime {
        _ = try? await createdRuntime.shutdown()
      }
      throw Self.failure(from: error)
    }
  }

  private static func failure(from error: Error) -> TeraRuntimeFailure {
    if case let TeraAppError.Failure(report) = error {
      return TeraRuntimeFailure(
        schemaVersion: report.schemaVersion,
        code: report.code,
        category: report.category,
        retryable: report.retryable,
        recoveryActions: report.recoveryActions,
        operationID: report.operationId,
        capabilityID: report.capabilityId,
        safeMessage: report.safeMessage
      )
    }
    if let failure = error as? TeraRuntimeFailure {
      return failure
    }
    return .local(
      operation: "generated.runtime",
      code: "ios.generated_runtime.unexpected",
      safeMessage: "The Tera runtime could not complete the operation."
    )
  }

  private static func verifiedMediaArtifact(
    _ record: FfiVerifiedMediaArtifactRecord,
    expectedArtifactID: String?,
    requiresOperationID: Bool
  ) throws -> TeraVerifiedMediaArtifact {
    guard record.schemaVersion == 1,
          !requiresOperationID || record.operationId?.isEmpty == false,
          expectedArtifactID == nil || expectedArtifactID == record.artifactId,
          let artifact = TeraVerifiedMediaArtifact(
            artifactID: record.artifactId,
            bytes: record.bytes,
            byteSize: record.byteSize,
            mediaType: record.mediaType,
            width: record.width,
            height: record.height
          )
    else {
      throw TeraRuntimeFailure.local(
        operation: "runtime.media.artifact",
        code: "ios.media.artifact_invalid",
        safeMessage: "The verified photo artifact is invalid."
      )
    }
    return artifact
  }
}

extension TeraRuntimeLaunchConfiguration {
  fileprivate func bootstrapSettingsReplacement(
    current: TeraMobileSettings
  ) -> TeraReplaceSettings {
    let environment = networkProfile.settingsValue
    var relays = writableRelays.map {
      TeraRelayPreference(url: $0, access: .readWrite)
    }
    if networkProfile != .simulator,
       !relays.contains(where: { $0.url == TeraNetworkValidator.canonicalRelay })
    {
      relays.insert(
        TeraRelayPreference(
          url: TeraNetworkValidator.canonicalRelay,
          access: .readWrite
        ),
        at: 0
      )
    }
    let authority = blossom?.endpointAuthority.settingsValue
      ?? current.blossomAuthority
    let primaryOrigin = blossom?.primaryOrigin ?? current.blossomPrimaryOrigin
    let fallbackOrigins = blossom?.fallbackOrigins ?? current.blossomFallbackOrigins
    return TeraReplaceSettings(
      expectedRevision: current.revision,
      networkEnvironment: environment,
      relays: relays,
      blossomAuthority: authority,
      blossomPrimaryOrigin: primaryOrigin,
      blossomFallbackOrigins: fallbackOrigins,
      allowCellularDownloads: current.allowCellularDownloads,
      allowCellularUploads: current.allowCellularUploads,
      allowBackgroundTransfers: current.allowBackgroundTransfers,
      mediaCacheBytes: current.mediaCacheBytes,
      mediaCacheArtifacts: current.mediaCacheArtifacts
    )
  }
}

extension TeraRuntimeNetworkProfile {
  fileprivate var settingsValue: TeraSettingsNetworkEnvironment {
    switch self {
    case .publicNetwork: .publicNetwork
    case .simulator: .simulator
    case .device: .physicalDevice
    }
  }
}

extension TeraBlossomEndpointAuthority {
  fileprivate var settingsValue: TeraBlossomAuthorityPreference {
    switch self {
    case .publicWebPKI: .publicWebPKI
    case .loopbackDevelopment: .loopbackDevelopment
    case .privateNetworkDevelopment: .privateNetworkDevelopment
    }
  }
}

extension TeraRuntimeSignerAvailability {
  fileprivate var generatedValue: SignerAvailabilityRecord {
    switch self {
    case .ready: .ready
    case .busy: .busy
    case .locked: .locked
    case .unavailable: .unavailable
    }
  }
}

extension HostSigningPurpose {
  fileprivate var appValue: TeraRuntimeSigningPurpose {
    switch self {
    case .nostrEvent: .nostrEvent
    case .blossomUpload: .blossomUpload
    }
  }
}

extension TeraRuntimeSigningOutcome {
  fileprivate var generatedOutcome: HostSigningOutcome {
    switch self {
    case .signed: .signed
    case .locked: .locked
    case .cancelled: .cancelled
    case .rejected: .rejected
    case .timedOut: .timedOut
    case .unavailable: .unavailable
    case .invalidated: .invalidated
    case .failed: .failed
    }
  }

  fileprivate var signatureHex: String? {
    if case let .signed(signatureHex) = self {
      return signatureHex
    }
    return nil
  }
}

extension TeraRuntimeClient {
  static func production() -> TeraRuntimeClient {
    TeraRuntimeClient { configuration in
      try await TeraGeneratedRuntimeBackend.start(configuration: configuration)
    }
  }
}

extension TeraProtectedDataState {
  fileprivate var generatedValue: ProtectedDataAvailability {
    switch self {
    case .available: .available
    case .unavailable: .unavailable
    }
  }
}

extension TeraBlossomHostKind {
  fileprivate var generatedValue: FfiBlossomHostKind {
    switch self {
    case .native: .native
    case .simulator: .simulator
    case .physicalDevice: .physicalDevice
    }
  }
}

extension TeraBlossomEndpointAuthority {
  fileprivate var generatedValue: FfiBlossomEndpointAuthority {
    switch self {
    case .publicWebPKI: .publicWebPki
    case .loopbackDevelopment: .loopbackDevelopment
    case .privateNetworkDevelopment: .privateNetworkDevelopment
    }
  }
}

extension FfiBlossomConfigurationRecord {
  fileprivate var appValue: TeraBlossomConfigurationStatus {
    TeraBlossomConfigurationStatus(
      schemaVersion: schemaVersion,
      hostKind: hostKind,
      endpointAuthority: endpointAuthority,
      primaryOrigin: primaryOrigin,
      fallbackOrigins: fallbackOrigins,
      configFingerprint: configFingerprint
    )
  }
}

extension FfiBlossomEvidenceRecord {
  fileprivate var appValue: TeraBlossomEvidence {
    TeraBlossomEvidence(
      schemaVersion: schemaVersion,
      origin: origin,
      configFingerprint: configFingerprint,
      state: state,
      lastSuccessfulState: lastSuccessfulState,
      transportSecurity: transportSecurity,
      observedAtUnixMilliseconds: observedAtUnixMs,
      httpStatus: httpStatus,
      errorCode: errorCode,
      serverErrorCode: serverErrorCode,
      errorPhase: errorPhase,
      retryable: retryable,
      possibleOrphan: possibleOrphan,
      attempts: attempts
    )
  }
}

extension FfiRuntimeChangeKind {
  fileprivate var appValue: TeraRuntimeChangeKind {
    switch self {
    case .initial: .initial
    case .identity: .identity
    case .settings: .settings
    case .profile: .profile
    case .today: .today
    case .drafts: .drafts
    case .relay: .relay
    case .media: .media
    case .lifecycle: .lifecycle
    }
  }
}

extension TeraLocalNetwork {
  fileprivate var generatedValue: FfiLocalNetworkRecord {
    FfiLocalNetworkRecord(
      schemaVersion: schemaVersion,
      id: id,
      label: label,
      relayUrls: relayURLs,
      locality: locality,
      followedAuthors: followedAuthors,
      generation: generation
    )
  }
}

extension TeraTodayProjectionUpdate {
  fileprivate var generatedValue: FfiTodayProjectionUpdate {
    switch self {
    case .incremental: .incremental
    case .rebuild: .rebuild
    }
  }
}

extension FfiTodayRefreshRecord {
  fileprivate var appValue: TeraTodayRefreshReceipt {
    TeraTodayRefreshReceipt(
      update: update.appValue,
      sourceEvents: sourceEvents,
      visibleCards: visibleCards,
      profiles: profiles,
      threadEntries: threadEntries,
      contentGeneration: contentGeneration,
      changed: changed
    )
  }
}

extension FfiTodayProjectionUpdate {
  fileprivate var appValue: TeraTodayProjectionUpdate {
    switch self {
    case .incremental: .incremental
    case .rebuild: .rebuild
    }
  }
}

extension FfiTodayPageRecord {
  fileprivate var appValue: TeraTodayPage {
    TeraTodayPage(
      asOfUnixSeconds: asOfUnixS,
      items: items.map(\.appValue),
      nextCursor: nextCursor
    )
  }
}

extension FfiTodayCardRecord {
  fileprivate var appValue: TeraTodayCard {
    TeraTodayCard(
      id: cardId,
      type: cardType.appValue,
      sourceEventID: sourceEventId,
      sourceAddress: sourceAddress,
      authorPublicKey: authorPublicKey,
      contractID: contractId,
      title: title,
      content: content,
      authoredAtUnixSeconds: authoredAtUnixS,
      effectiveAtUnixSeconds: effectiveAtUnixS,
      eventStartUnixSeconds: eventStartUnixS,
      eventEndUnixSeconds: eventEndUnixS,
      location: location,
      priceAmount: priceAmount,
      priceCurrency: priceCurrency,
      priceUnit: priceUnit,
      quantity: quantity,
      foodSummary: foodSummary,
      foodPublishedAtUnixSeconds: foodPublishedAtUnixS,
      foodStatus: foodStatus,
      contextRank: contextRank,
      inclusionReason: inclusionReason,
      media: media.map(\.appValue),
      lifecycle: lifecycle.appValue,
      rankDigest: rankDigest,
      authorProfile: authorProfile?.appValue,
      thread: thread.map(\.appValue),
      localOperationID: localOperationId,
      localOperationState: localOperationState
    )
  }
}

extension FfiTodayCardType {
  fileprivate var appValue: TeraTodayCardType {
    switch self {
    case .update: .update
    case .photoUpdate: .photoUpdate
    case .ask: .ask
    case .event: .event
    case .foodAvailability: .foodAvailability
    }
  }
}

extension FfiMediaReferenceRecord {
  fileprivate var appValue: TeraMediaReference {
    TeraMediaReference(
      referenceFingerprint: referenceFingerprint,
      url: url,
      sha256: sha256,
      mediaType: mediaType,
      width: width,
      height: height,
      byteSize: byteSize,
      alt: alt,
      verification: verification.appValue
    )
  }
}

extension FfiMediaVerificationState {
  fileprivate var appValue: TeraMediaVerificationState {
    switch self {
    case .pending: .pending
    case .verified: .verified
    case .failed: .failed
    case .unavailable: .unavailable
    }
  }
}

extension FfiProfileRecord {
  fileprivate var appValue: TeraProfileSummary {
    TeraProfileSummary(
      authorPublicKey: authorPublicKey,
      name: name,
      displayName: displayName,
      about: about,
      picture: picture?.appValue,
      banner: banner?.appValue,
      nip05: nip05,
      website: website,
      lightningAddress: lightningAddress
    )
  }
}

extension FfiSearchResultRecord {
  fileprivate var appValue: TeraSearchResult {
    TeraSearchResult(
      type: resultType.appValue,
      id: stableId,
      card: card?.appValue,
      profile: profile?.appValue
    )
  }
}

extension FfiSearchResultType {
  fileprivate var appValue: TeraSearchResultType {
    switch self {
    case .card: .card
    case .profile: .profile
    }
  }
}

extension FfiMeRecord {
  fileprivate var appValue: TeraMeSnapshot {
    TeraMeSnapshot(
      publicKey: publicKey,
      profile: profile?.appValue,
      cards: cards.map(\.appValue)
    )
  }
}

extension FfiThreadEntryRecord {
  fileprivate var appValue: TeraThreadEntry {
    TeraThreadEntry(
      id: eventId,
      authorPublicKey: authorPublicKey,
      content: content,
      authoredAtUnixSeconds: authoredAtUnixS,
      type: profile.appValue,
      root: root,
      parentEventID: parentEventId,
      authorProfile: authorProfile?.appValue
    )
  }
}

extension FfiThreadProfile {
  fileprivate var appValue: TeraThreadEntryType {
    switch self {
    case .profile: .profile
    case .reply: .reply
    case .comment: .comment
    case .deletion: .deletion
    }
  }
}

extension FfiCardLifecycleState {
  fileprivate var appValue: TeraCardLifecycleState {
    switch self {
    case .active: .active
    case .sold: .sold
    case .past: .past
    }
  }
}

extension FfiAddSchemaRecord {
  fileprivate var appValue: TeraAddSchema {
    TeraAddSchema(
      schemaVersion: schemaVersion,
      commandType: commandType.appValue,
      label: label,
      fields: fields.map(\.appValue)
    )
  }
}

extension FfiAddFieldRecord {
  fileprivate var appValue: TeraAddField {
    TeraAddField(
      schemaVersion: schemaVersion,
      id: id,
      label: label,
      kind: kind.appValue,
      required: required,
      choices: choices,
      maxBytes: maxBytes,
      maxItems: maxItems
    )
  }
}

extension FfiAddFieldKind {
  fileprivate var appValue: TeraAddFieldKind {
    switch self {
    case .text: .text
    case .multilineText: .multilineText
    case .date: .date
    case .dateTime: .dateTime
    case .decimal: .decimal
    case .choice: .choice
    case .location: .location
    case .media: .media
    }
  }
}

extension FfiAddCommandType {
  fileprivate var appValue: TeraAddCommandType {
    switch self {
    case .createUpdate: .createUpdate
    case .createPhotoUpdate: .createPhotoUpdate
    case .createAsk: .createAsk
    case .createEvent: .createEvent
    case .createFoodAvailability: .createFoodAvailability
    }
  }
}

extension TeraAddCommandType {
  fileprivate var generatedValue: FfiAddCommandType {
    switch self {
    case .createUpdate: .createUpdate
    case .createPhotoUpdate: .createPhotoUpdate
    case .createAsk: .createAsk
    case .createEvent: .createEvent
    case .createFoodAvailability: .createFoodAvailability
    }
  }
}

extension FfiEventTimingKind {
  fileprivate var appValue: TeraEventTiming {
    switch self {
    case .allDay: .allDay
    case .timed: .timed
    }
  }
}

extension TeraEventTiming {
  fileprivate var generatedValue: FfiEventTimingKind {
    switch self {
    case .allDay: .allDay
    case .timed: .timed
    }
  }
}

extension TeraAddRuntimeInput {
  fileprivate var generatedValue: FfiAddDraftInput {
    FfiAddDraftInput(
      schemaVersion: 1,
      commandType: form.commandType.generatedValue,
      content: form.content,
      identifier: form.identifier,
      title: form.title,
      summary: form.summary,
      location: form.location,
      eventTiming: form.eventTiming?.generatedValue,
      eventStartDate: form.eventStartDate,
      eventEndDate: form.eventEndDate,
      eventStartUnixS: form.eventStartUnixSeconds,
      eventEndUnixS: form.eventEndUnixSeconds,
      eventTimezone: form.eventTimezone,
      priceAmount: form.priceAmount,
      currency: form.currency,
      unit: form.unit,
      quantity: form.quantity,
      foodPublishedAtUnixS: form.foodPublishedAtUnixSeconds,
      foodStatus: form.foodStatus,
      media: media.map(\.generatedValue)
    )
  }
}

extension TeraPreparedMediaHandle {
  fileprivate var generatedValue: FfiPreparedMediaInput {
    FfiPreparedMediaInput(
      schemaVersion: 1,
      opaqueReference: media.opaqueReference,
      fileDescriptor: fileDescriptor,
      sha256: media.sha256,
      mediaType: media.mediaType,
      byteSize: media.byteSize,
      width: media.width,
      height: media.height,
      alt: media.alt,
      preparedAtUnixS: media.preparedAtUnixSeconds
    )
  }
}

extension FfiIdentityLockState {
  fileprivate var appValue: TeraSettingsIdentityLockState {
    switch self {
    case .locked: .locked
    case .unlocked: .unlocked
    }
  }
}

extension FfiSettingsIdentityRecord {
  fileprivate var appValue: TeraSettingsIdentity {
    TeraSettingsIdentity(id: id, publicKeyHex: publicKey)
  }
}

extension FfiIdentityStateRecord {
  fileprivate var appValue: TeraSettingsIdentityState {
    TeraSettingsIdentityState(
      identities: identities.map(\.appValue),
      activeIdentityID: activeIdentityId,
      lockState: lockState.appValue,
      pendingImportOperationID: pendingImportOperationId
    )
  }
}

extension FfiMobileNetworkEnvironment {
  fileprivate var appValue: TeraSettingsNetworkEnvironment {
    switch self {
    case .public: .publicNetwork
    case .simulator: .simulator
    case .physicalDevice: .physicalDevice
    }
  }
}

extension TeraSettingsNetworkEnvironment {
  fileprivate var generatedValue: FfiMobileNetworkEnvironment {
    switch self {
    case .publicNetwork: .public
    case .simulator: .simulator
    case .physicalDevice: .physicalDevice
    }
  }
}

extension FfiRelayAccessPreference {
  fileprivate var appValue: TeraRelayAccessPreference {
    switch self {
    case .readOnly: .readOnly
    case .readWrite: .readWrite
    }
  }
}

extension TeraRelayAccessPreference {
  fileprivate var generatedValue: FfiRelayAccessPreference {
    switch self {
    case .readOnly: .readOnly
    case .readWrite: .readWrite
    }
  }
}

extension FfiBlossomAuthorityPreference {
  fileprivate var appValue: TeraBlossomAuthorityPreference {
    switch self {
    case .publicWebPki: .publicWebPKI
    case .loopbackDevelopment: .loopbackDevelopment
    case .privateNetworkDevelopment: .privateNetworkDevelopment
    }
  }
}

extension TeraBlossomAuthorityPreference {
  fileprivate var generatedValue: FfiBlossomAuthorityPreference {
    switch self {
    case .publicWebPKI: .publicWebPki
    case .loopbackDevelopment: .loopbackDevelopment
    case .privateNetworkDevelopment: .privateNetworkDevelopment
    }
  }
}

extension FfiMobileSettingsRecord {
  fileprivate var appValue: TeraMobileSettings {
    TeraMobileSettings(
      revision: revision,
      identity: identity.appValue,
      networkEnvironment: relays.environment.appValue,
      relays: relays.endpoints.map {
        TeraRelayPreference(url: $0.url, access: $0.access.appValue)
      },
      blossomAuthority: blossom.authority.appValue,
      blossomPrimaryOrigin: blossom.primaryOrigin,
      blossomFallbackOrigins: blossom.fallbackOrigins,
      allowCellularDownloads: mediaNetwork.allowCellularDownloads,
      allowCellularUploads: mediaNetwork.allowCellularUploads,
      allowBackgroundTransfers: mediaNetwork.allowBackgroundTransfers,
      mediaCacheBytes: localStorage.mediaCacheBytes,
      mediaCacheArtifacts: localStorage.mediaCacheArtifacts
    )
  }
}

extension TeraReplaceSettings {
  fileprivate var generatedValue: FfiReplaceSettingsRecord {
    let environment = networkEnvironment.generatedValue
    return FfiReplaceSettingsRecord(
      schemaVersion: 1,
      expectedRevision: expectedRevision,
      relays: FfiRelayPreferencesRecord(
        schemaVersion: 1,
        environment: environment,
        endpoints: relays.map {
          FfiRelayPreferenceRecord(
            schemaVersion: 1,
            url: $0.url,
            access: $0.access.generatedValue
          )
        }
      ),
      blossom: FfiBlossomPreferencesRecord(
        schemaVersion: 1,
        environment: environment,
        authority: blossomAuthority.generatedValue,
        primaryOrigin: blossomPrimaryOrigin,
        fallbackOrigins: blossomFallbackOrigins
      ),
      mediaNetwork: FfiMediaNetworkPolicyRecord(
        schemaVersion: 1,
        allowCellularDownloads: allowCellularDownloads,
        allowCellularUploads: allowCellularUploads,
        allowBackgroundTransfers: allowBackgroundTransfers
      ),
      localStorage: FfiLocalStoragePolicyRecord(
        schemaVersion: 1,
        mediaCacheBytes: mediaCacheBytes,
        mediaCacheArtifacts: mediaCacheArtifacts
      )
    )
  }
}

extension FfiSettingsTransitionRecord {
  fileprivate var appValue: TeraSettingsTransition {
    TeraSettingsTransition(
      settings: settings.appValue,
      runtimeRestartRequired: runtimeRestartRequired,
      outboxRequeueRequired: outboxRequeueRequired,
      mediaCacheInvalidationRequired: mediaCacheInvalidationRequired
    )
  }
}

extension TeraIdentityCommandKind {
  fileprivate var generatedValue: FfiIdentityCommandKind {
    switch self {
    case .beginImport: .beginImport
    case .completeImport: .completeImport
    case .cancelImport: .cancelImport
    case .select: .select
    case .lock: .lock
    case .unlock: .unlock
    case .recover: .recover
    }
  }
}

extension TeraIdentityCommand {
  fileprivate var generatedValue: FfiIdentityCommandRecord {
    FfiIdentityCommandRecord(
      schemaVersion: 1,
      kind: kind.generatedValue,
      operationId: operationID,
      identityId: identityID,
      publicKey: publicKeyHex
    )
  }
}

extension TeraProfileMetadataInput {
  fileprivate var generatedValue: FfiProfileMetadataInputRecord {
    FfiProfileMetadataInputRecord(
      schemaVersion: 1,
      name: name,
      displayName: displayName,
      about: about,
      picture: picture?.generatedValue,
      banner: banner?.generatedValue,
      nip05: nip05,
      bot: bot
    )
  }
}

extension FfiProfileStatusRecord {
  fileprivate var appValue: TeraProfileStatus {
    TeraProfileStatus(
      id: operationId,
      revision: revision,
      authorPublicKey: authorPublicKey,
      state: state.appValue,
      deliveryID: deliveryId,
      createdAtUnixMilliseconds: createdAtUnixMs,
      updatedAtUnixMilliseconds: updatedAtUnixMs,
      settlement: settlement?.appValue
    )
  }
}

extension FfiDraftFormMediaRecord {
  fileprivate var appValue: TeraPreparedMedia {
    TeraPreparedMedia(
      opaqueReference: opaqueReference,
      remoteURL: url,
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

extension FfiDraftFormRecord {
  fileprivate var appValue: TeraAddForm {
    TeraAddForm(
      commandType: commandType.appValue,
      content: content,
      identifier: identifier,
      title: title,
      summary: summary,
      location: location,
      eventTiming: eventTiming?.appValue,
      eventStartDate: eventStartDate,
      eventEndDate: eventEndDate,
      eventStartUnixSeconds: eventStartUnixS,
      eventEndUnixSeconds: eventEndUnixS,
      eventTimezone: eventTimezone,
      priceAmount: priceAmount,
      currency: currency,
      unit: unit,
      quantity: quantity,
      foodPublishedAtUnixSeconds: foodPublishedAtUnixS,
      foodStatus: foodStatus,
      media: media.map(\.appValue)
    )
  }
}

extension FfiDraftStatusRecord {
  fileprivate var appValue: TeraDraftStatus {
    TeraDraftStatus(
      id: draftId,
      revision: revision,
      authorPublicKey: authorPublicKey,
      kind: kind.appValue,
      commandType: commandType.appValue,
      form: form?.appValue,
      state: state.appValue,
      cardID: cardId,
      operationID: operationId,
      createdAtUnixMilliseconds: createdAtUnixMs,
      updatedAtUnixMilliseconds: updatedAtUnixMs,
      media: media.map(\.appValue),
      settlement: settlement?.appValue,
      isRevision: isRevision
    )
  }
}

extension FfiRevisionStatusRecord {
  fileprivate var appValue: TeraRevisionStatus {
    TeraRevisionStatus(
      operationID: operationId,
      replacement: replacement.appValue,
      retraction: retraction?.appValue,
      policy: policy.appValue,
      phase: phase.appValue
    )
  }
}

extension FfiRevisionPolicy {
  fileprivate var appValue: TeraRevisionPolicy {
    switch self {
    case .replaceThenRetract: .replaceThenRetract
    case .addressableReplacement: .addressableReplacement
    }
  }
}

extension FfiRevisionPhase {
  fileprivate var appValue: TeraRevisionPhase {
    switch self {
    case .replacementPending: .replacementPending
    case .replacementFailed: .replacementFailed
    case .retractionPending: .retractionPending
    case .complete: .complete
    case .partialEffect: .partialEffect
    case .cancelled: .cancelled
    }
  }
}

extension FfiRelayAccessRecord {
  fileprivate var appValue: TeraRelayAccess {
    switch self {
    case .readOnly: .readOnly
    case .readWrite: .readWrite
    }
  }
}

extension FfiDraftKind {
  fileprivate var appValue: TeraDraftKind {
    switch self {
    case .add: .add
    case .retraction: .retraction
    }
  }
}

extension FfiOutboxState {
  fileprivate var appValue: TeraOutboxState {
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

extension FfiDraftMediaRecord {
  fileprivate var appValue: TeraDraftMediaStatus {
    TeraDraftMediaStatus(
      url: url,
      stage: stage.appValue,
      uploadAttempts: uploadAttempts,
      verifiedAtUnixMilliseconds: verifiedAtUnixMs,
      possibleOrphan: possibleOrphan,
      orphanReasonCode: orphanReasonCode,
      orphanRecordedAtUnixMilliseconds: orphanRecordedAtUnixMs
    )
  }
}

extension FfiMediaStage {
  fileprivate var appValue: TeraDraftMediaStage {
    switch self {
    case .pending: .pending
    case .preparing: .preparing
    case .uploading: .uploading
    case .verified: .verified
    case .failed: .failed
    case .orphaned: .orphaned
    }
  }
}

extension FfiOperationSettlementRecord {
  fileprivate var appValue: TeraOperationSettlement {
    TeraOperationSettlement(
      artifacts: artifacts,
      signed: signed,
      admitted: admitted,
      pending: pending,
      retryable: retryable,
      indeterminate: indeterminate,
      failedTerminal: failedTerminal,
      cancelled: cancelled,
      deliveryPlans: deliveryPlans,
      deliverySatisfied: deliverySatisfied,
      deliveryPending: deliveryPending,
      deliveryRetryable: deliveryRetryable,
      deliveryExhausted: deliveryExhausted,
      deliveryFailedTerminal: deliveryFailedTerminal,
      deliveryCancelled: deliveryCancelled
    )
  }
}

extension TeraRetractionDraftInput {
  fileprivate var generatedValue: FfiRetractionDraftInput {
    FfiRetractionDraftInput(
      schemaVersion: 1,
      commandType: commandType.generatedValue,
      targetCardId: targetCardID,
      targetEventId: targetEventID,
      targetKind: targetKind,
      targetAddress: targetAddress,
      reason: reason
    )
  }
}

extension TeraBlossomUploadIntent {
  fileprivate var generatedValue: FfiBlossomUploadIntent {
    FfiBlossomUploadIntent(
      schemaVersion: 1,
      draftId: draftID,
      expectedRevision: expectedRevision,
      media: media.generatedValue
    )
  }
}

extension FfiNativeUploadJobRecord {
  fileprivate var appValue: TeraNativeUploadJob {
    TeraNativeUploadJob(
      operationID: operationId,
      draft: draft.appValue,
      remoteURL: remoteUrl,
      authorizationHeader: authorizationHeader,
      expectedSHA256: expectedSha256,
      mediaType: mediaType,
      byteSize: byteSize
    )
  }
}

extension TeraNativeUploadCompletion {
  fileprivate var generatedValue: FfiNativeUploadCompletionInput {
    FfiNativeUploadCompletionInput(
      schemaVersion: 1,
      draftId: draftID,
      expectedRevision: expectedRevision,
      media: media.generatedValue,
      statusCode: statusCode,
      responseMediaType: responseMediaType,
      responseContentEncoding: responseContentEncoding,
      responseBody: responseBody
    )
  }
}
