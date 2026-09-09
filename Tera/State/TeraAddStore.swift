import Foundation

enum TeraAddLoadState: Sendable, Equatable {
  case idle
  case loading
  case ready
  case failed(String)
}

@MainActor
final class TeraAddStore: ObservableObject {
  @Published private(set) var schemas: [TeraAddSchema] = []
  @Published private(set) var drafts: [TeraDraftStatus] = []
  @Published private(set) var activeDraft: TeraDraftStatus?
  @Published private(set) var form: TeraAddForm
  @Published private(set) var state: TeraAddLoadState = .idle
  @Published private(set) var mediaSupport: TeraAddMediaSupport = .unavailable
  @Published private(set) var blossomConfiguration: TeraBlossomConfigurationStatus?
  @Published private(set) var blossomEvidence: TeraBlossomEvidence?
  @Published private(set) var isCheckingBlossom = false
  @Published private(set) var isWorking = false
  @Published private(set) var message: String?
  @Published private(set) var lastFailureCode: String?
  @Published private(set) var observationState: TeraRuntimeObservationState = .inactive

  private let runtimeClient: TeraRuntimeClient
  private let media: (any TeraAddMediaHandling)?
  private let observationDelay: @Sendable (UInt32) async throws -> Void
  private let identifier: @Sendable () -> String
  private let clock: TeraClock
  private var generation = TeraSessionGeneration.initial
  private var operationGeneration: TeraSessionGeneration?
  private var operationTask: Task<Void, Never>?
  private let observation = TeraStoreObservation()
  private var configuration: TeraPresentationConfiguration?
  private var draftsGeneration = TeraSessionGeneration.initial
  private var probeGeneration = TeraSessionGeneration.initial
  private var blossomGeneration = TeraSessionGeneration.initial
  private var revisionTarget: TeraRevisionTarget?
  private var revisionOperationID: String?
  private var activePublicKey: String?

  init(
    runtimeClient: TeraRuntimeClient,
    media: (any TeraAddMediaHandling)? = nil,
    initialType: TeraAddCommandType = .createUpdate,
    identifier: @escaping @Sendable () -> String = {
      UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()
    },
    clock: TeraClock = .system,
    observationDelay: @escaping @Sendable (UInt32) async throws -> Void =
      TeraRuntimeObservationBackoff.sleep
  ) {
    self.runtimeClient = runtimeClient
    self.media = media
    self.identifier = identifier
    self.clock = clock
    self.observationDelay = observationDelay
    form = TeraAddPresentation.newForm(
      type: initialType,
      identifier: identifier,
      clock: clock
    )
  }

  var selectedSchema: TeraAddSchema? {
    schemas.first(where: { $0.commandType == form.commandType })
  }

  var isFormEditable: Bool {
    guard activeDraft?.isRevision != true, activeDraft?.kind != .retraction else { return false }
    return activeDraft?.state.isEditable ?? true
  }

  var isProductReady: Bool {
    state == .ready && selectedSchema != nil
  }

  var canSave: Bool {
    isProductReady && isFormEditable && !isWorking
  }

  var canSubmit: Bool {
    isProductReady && !isWorking
      && (activeDraft?.isRevision == true || activeDraft?.state.canAdvance == true
        || isFormEditable)
  }

  var acceptsMedia: Bool {
    mediaLimit > 0
  }

  var canAddMedia: Bool {
    isFormEditable && acceptsMedia && form.media.count < mediaLimit
  }

  private var mediaLimit: Int {
    guard
      let maximum = selectedSchema?.fields
        .first(where: { $0.kind == .media })?.maxItems
    else { return 0 }
    return Int(maximum)
  }

  func configure(snapshot: TeraRuntimeSnapshot) {
    let updated = TeraPresentationConfiguration(snapshot: snapshot)
    if let configuration, configuration != updated {
      stop()
      schemas = []
      drafts = []
      activeDraft = nil
      revisionTarget = nil
      revisionOperationID = nil
      form = TeraAddPresentation.newForm(type: form.commandType, identifier: identifier, clock: clock)
      state = .idle
      mediaSupport = .unavailable
      message = nil
      lastFailureCode = nil
    }
    blossomGeneration = blossomGeneration.invalidated()
    configuration = updated
    activePublicKey = snapshot.identity.publicKeyHex
    blossomConfiguration = snapshot.blossomConfiguration
    blossomEvidence = snapshot.blossomEvidence
  }

  func start() async {
    guard !observation.isActive, !Task.isCancelled else { return }
    startObservation()
    guard operationGeneration == nil else { return }
    message = nil
    state = .loading
    generation = generation.invalidated()
    let requestedGeneration = generation
    draftsGeneration = draftsGeneration.invalidated()
    let draftRequest = draftsGeneration
    let serviceRequest = blossomGeneration
    do {
      async let schemaResult = runtimeClient.addSchemas()
      async let draftResult = runtimeClient.draftHeads(limit: 100)
      async let supportResult = loadMediaSupport()
      let (loadedSchemas, loadedDrafts, loadedSupport) = try await (
        schemaResult,
        draftResult,
        supportResult
      )
      guard isCurrent(requestedGeneration) else { return }
      if draftRequest == draftsGeneration {
        try await media?.reconcileBackgroundUploads(drafts: loadedDrafts)
      }
      try ensureCurrent(requestedGeneration)
      schemas = try TeraProductSurfaceContract.validate(schemas: loadedSchemas)
      if draftRequest == draftsGeneration {
        drafts = TeraAddPresentation.sorted(loadedDrafts)
      }
      if serviceRequest == blossomGeneration {
        mediaSupport = loadedSupport
      }
      state = .ready
    } catch {
      guard isCurrent(requestedGeneration) else { return }
      state = .failed(TeraAddPresentation.message(for: error))
    }
  }

  func stop() {
    generation = generation.invalidated()
    operationGeneration = nil
    probeGeneration = probeGeneration.invalidated()
    isCheckingBlossom = false
    operationTask?.cancel()
    operationTask = nil
    observation.stop()
    observationState = .stopped
    isWorking = false
  }

  func suspend() {
    observation.stop()
    observationState = .stopped
  }

  func selectType(_ type: TeraAddCommandType) {
    guard !isWorking, isFormEditable, form.commandType != type else { return }
    generation = generation.invalidated()
    activeDraft = nil
    revisionTarget = nil
    revisionOperationID = nil
    form = TeraAddPresentation.newForm(
      type: type,
      identifier: identifier,
      clock: clock
    )
    message = nil
  }

  func updateForm<Value>(_ keyPath: WritableKeyPath<TeraAddForm, Value>, _ value: Value) {
    guard isFormEditable else { return }
    generation = generation.invalidated()
    form[keyPath: keyPath] = value
  }

  func newDraft(type: TeraAddCommandType? = nil) {
    guard !isWorking else { return }
    generation = generation.invalidated()
    activeDraft = nil
    revisionTarget = nil
    revisionOperationID = nil
    form = TeraAddPresentation.newForm(
      type: type ?? form.commandType,
      identifier: identifier,
      clock: clock
    )
    message = nil
  }

  func reopen(_ draft: TeraDraftStatus) {
    guard !isWorking else { return }
    guard let snapshot = draft.form else {
      message = "This operation has no editable Add form."
      return
    }
    generation = generation.invalidated()
    activeDraft = draft
    revisionTarget = nil
    revisionOperationID = draft.isRevision ? draft.id : nil
    form = snapshot
    message = draft.state.isEditable ? "Draft reopened." : draft.honestSummary
  }

  func importPhotos() async {
    guard canAddMedia, let media else {
      message = "Photo intake is unavailable."
      return
    }
    await perform { requestedGeneration in
      let remaining = self.mediaLimit - self.form.media.count
      guard remaining > 0 else { return }
      let imported = try await media.importImages(limit: remaining)
      try self.ensureCurrent(requestedGeneration)
      self.form.media.append(contentsOf: imported.prefix(remaining))
      self.message = "Photo prepared. Add descriptive text before publishing."
    }
  }

  func checkPhotoService() async {
    guard !isCheckingBlossom, !Task.isCancelled else { return }
    guard blossomConfiguration != nil else {
      mediaSupport = .unavailable
      message = "No photo service is configured for the current network profile."
      return
    }
    probeGeneration = probeGeneration.invalidated()
    let probe = probeGeneration
    blossomGeneration = blossomGeneration.invalidated()
    let serviceRequest = blossomGeneration
    let requestedGeneration = generation
    isCheckingBlossom = true
    defer {
      if probe == probeGeneration {
        isCheckingBlossom = false
      }
    }
    do {
      let evidence = try await runtimeClient.probeBlossom()
      try ensureCurrent(requestedGeneration)
      let support = try await loadMediaSupport()
      try ensureCurrent(requestedGeneration)
      guard probe == probeGeneration, serviceRequest == blossomGeneration else { return }
      blossomEvidence = evidence
      mediaSupport = support
      message = "Photo service is reachable."
    } catch {
      guard isCurrent(requestedGeneration), probe == probeGeneration,
            serviceRequest == blossomGeneration else { return }
      guard await refreshBlossomSnapshot(), isCurrent(requestedGeneration),
            probe == probeGeneration else { return }
      mediaSupport = .unavailable
      message = TeraAddPresentation.message(for: error)
    }
  }

  func capturePhoto() async {
    guard canAddMedia, let media else {
      message = "Camera intake is unavailable."
      return
    }
    await perform { requestedGeneration in
      let captured = try await media.captureImage()
      try self.ensureCurrent(requestedGeneration)
      guard self.form.media.count < self.mediaLimit else { return }
      self.form.media.append(captured)
      self.message = "Photo prepared. Add descriptive text before publishing."
    }
  }

  func removeMedia(id: String) {
    guard isFormEditable else { return }
    generation = generation.invalidated()
    form.media.removeAll(where: { $0.id == id })
  }

  func updateMediaAlt(id: String, alt: String) {
    guard isFormEditable,
      let index = form.media.firstIndex(where: { $0.id == id })
    else { return }
    generation = generation.invalidated()
    form.media[index].alt = alt
  }

  func save() async {
    await perform { requestedGeneration in
      _ = try await self.saveCurrentForm(generation: requestedGeneration)
      try self.ensureCurrent(requestedGeneration)
      self.message = "Draft saved on this device."
    }
  }

  func submit() async {
    await perform { requestedGeneration in
      var status: TeraDraftStatus =
        if let active = self.activeDraft, !active.state.isEditable {
          active
        } else {
          try await self.saveCurrentForm(generation: requestedGeneration)
        }

      try self.ensureCurrent(requestedGeneration)
      if !status.media.isEmpty,
        status.media.contains(where: { $0.stage != .verified })
      {
        status = try await self.uploadPendingMedia(status, generation: requestedGeneration)
      }

      try self.ensureCurrent(requestedGeneration)
      if status.isRevision {
        let revision = try await self.runtimeClient.advanceRevision(
          operationID: self.revisionOperationID ?? status.id
        )
        try self.accept(revision, generation: requestedGeneration)
        self.message = revision.honestSummary
        return
      }

      if status.state.isEditable || status.state == .readyToSign {
        do {
          status = try await self.runtimeClient.queueAddIntent(
            id: status.id,
            expectedRevision: status.revision
          )
          try self.accept(status, generation: requestedGeneration)
        } catch {
          try self.ensureCurrent(requestedGeneration)
          if TeraAddPresentation.failure(for: error)?.code == "writable_relay_unavailable" {
            try self.accept(status, generation: requestedGeneration)
            self.message =
              status.media.isEmpty
              ? "Draft saved. Configure a writable relay to publish."
              : "Photo verified and draft saved. Configure a writable relay to publish."
            return
          }
          throw error
        }
      }

      try await self.advanceSubmittedDraft(status, generation: requestedGeneration)
    }
  }

  func retry(_ draft: TeraDraftStatus? = nil) async {
    guard let draft = draft ?? activeDraft else { return }
    await perform { requestedGeneration in
      var current = try await self.runtimeClient.draftStatus(id: draft.id)
      try self.ensureCurrent(requestedGeneration)
      if current.state == .draft || current.state == .mediaPreparing
        || current.state == .readyToSign
      {
        self.activeDraft = current
        self.revisionOperationID = current.isRevision ? current.id : nil
        if let form = current.form {
          self.form = form
        }
        self.message = "Review the draft before submitting again."
        return
      }
      if current.isRevision {
        let revision = try await self.runtimeClient.advanceRevision(
          operationID: current.id
        )
        try self.accept(revision, generation: requestedGeneration)
        self.message = revision.honestSummary
        return
      }
      if current.state.canAdvance {
        current = try await self.runtimeClient.advanceDraft(
          id: current.id,
          expectedRevision: current.revision
        )
      }
      try self.accept(current, generation: requestedGeneration)
      self.message = current.honestSummary
    }
  }

  func cancel(_ draft: TeraDraftStatus? = nil) async {
    guard let draft = draft ?? activeDraft, draft.state.canCancel else { return }
    await perform { requestedGeneration in
      let current = try await self.runtimeClient.draftStatus(id: draft.id)
      try self.ensureCurrent(requestedGeneration)
      if current.isRevision {
        let cancelled = try await self.runtimeClient.cancelRevision(
          operationID: current.id
        )
        try self.accept(cancelled, generation: requestedGeneration)
        self.message = cancelled.honestSummary
        return
      }
      let cancelled = try await self.runtimeClient.cancelAddIntent(
        id: current.id,
        expectedRevision: current.revision
      )
      try self.accept(cancelled, generation: requestedGeneration)
      self.message = "Local work was cancelled. Any already-published relay effect is preserved."
    }
  }

  func retractAndRevise(_ card: TeraTodayCard) async {
    await perform { requestedGeneration in
      guard let publicKey = self.activePublicKey, publicKey == card.authorPublicKey else {
        throw TeraRuntimeFailure.local(
          operation: "add.revise",
          code: "ios.add.revision_not_authorized",
          safeMessage: "Only your own post can be revised."
        )
      }
      guard let operationID = card.localOperationID else {
        throw TeraRuntimeFailure.local(
          operation: "add.revise",
          code: "ios.add.revision_source_unavailable",
          safeMessage: "This post cannot be revised losslessly on this device."
        )
      }
      let source = try await self.runtimeClient.draftStatus(id: operationID)
      try self.ensureCurrent(requestedGeneration)
      guard let sourceForm = source.form else {
        throw TeraRuntimeFailure.local(
          operation: "add.revise",
          code: "ios.add.revision_form_unavailable",
          safeMessage: "The original Add form is unavailable on this device."
        )
      }
      self.revisionTarget = TeraRevisionTarget(
        cardID: card.id,
        sourceEventID: card.sourceEventID,
        sourceAddress: card.sourceAddress,
        authorPublicKey: card.authorPublicKey
      )
      self.revisionOperationID = nil
      self.activeDraft = nil
      self.form = sourceForm
      self.message = "Review the lossless revised copy before publishing."
    }
  }

  func retract(_ card: TeraTodayCard) async {
    await perform { requestedGeneration in
      guard let publicKey = self.activePublicKey, publicKey == card.authorPublicKey else {
        throw TeraRuntimeFailure.local(
          operation: "add.retract",
          code: "ios.add.retraction_not_authorized",
          safeMessage: "Only your own post can be retracted."
        )
      }
      guard let targetKind = card.retractionTargetKind else {
        throw TeraRuntimeFailure.local(
          operation: "add.retract",
          code: "ios.add.retraction_target_invalid",
          safeMessage: "This post cannot be retracted safely."
        )
      }
      let draftID = self.identifier()
      guard TeraAddPresentation.isValidIdentifier(draftID) else {
        throw TeraRuntimeFailure.local(
          operation: "add.retract",
          code: "ios.add.identifier_invalid",
          safeMessage: "The local operation identifier is invalid."
        )
      }
      var status = try await self.runtimeClient.saveRetractionDraft(
        id: draftID,
        input: TeraRetractionDraftInput(
          commandType: card.type.addCommandType,
          targetCardID: card.id,
          targetEventID: card.sourceEventID,
          targetKind: targetKind,
          targetAddress: card.sourceAddress,
          reason: "Removed by author."
        ),
        authoredAtUnixSeconds: self.clock.unixSeconds(),
        persistedAtUnixMilliseconds: self.clock.unixMilliseconds()
      )
      try self.accept(status, generation: requestedGeneration)
      status = try await self.runtimeClient.queueAddIntent(
        id: status.id,
        expectedRevision: status.revision
      )
      try self.accept(status, generation: requestedGeneration)
      if status.state.canAdvance {
        status = try await self.runtimeClient.advanceDraft(
          id: status.id,
          expectedRevision: status.revision
        )
        try self.accept(status, generation: requestedGeneration)
      }
      self.message = status.honestSummary
    }
  }

  private func advanceSubmittedDraft(
    _ initial: TeraDraftStatus, generation requestedGeneration: TeraSessionGeneration
  ) async throws {
    var status = initial
    do {
      if status.state.canAdvance {
        status = try await runtimeClient.advanceDraft(
          id: status.id,
          expectedRevision: status.revision
        )
        try accept(status, generation: requestedGeneration)
      }
      message = status.honestSummary
    } catch {
      try ensureCurrent(requestedGeneration)
      // Queueing is the commit point. A later retry must reuse this immutable snapshot.
      message = "Saved for retry. \(TeraAddPresentation.message(for: error))"
    }
  }

  private func saveCurrentForm(generation requestedGeneration: TeraSessionGeneration) async throws -> TeraDraftStatus {
    guard isFormEditable else {
      throw TeraRuntimeFailure.local(
        operation: "add.save",
        code: "ios.add.form_frozen",
        safeMessage: "Submitted drafts cannot be changed. Create a revised copy instead."
      )
    }
    let opened = try await openedMedia()
    defer { opened.close() }
    try ensureCurrent(requestedGeneration)
    let input = TeraAddRuntimeInput(form: form, media: opened.handles)
    if let target = revisionTarget {
      let revision = try await runtimeClient.saveRevisionIntent(
        target: target,
        replacement: input
      )
      try ensureCurrent(requestedGeneration)
      revisionOperationID = revision.operationID
      revisionTarget = nil
      try accept(revision, generation: requestedGeneration)
      return revision.replacement
    }
    let status = try await runtimeClient.saveAddIntent(
      input: input,
      existingDraftID: activeDraft?.isRevision == true ? nil : activeDraft?.id,
      expectedRevision: activeDraft?.isRevision == true ? nil : activeDraft?.revision
    )
    try accept(status, generation: requestedGeneration)
    return status
  }

  private func uploadPendingMedia(
    _ initial: TeraDraftStatus, generation requestedGeneration: TeraSessionGeneration
  ) async throws
    -> TeraDraftStatus
  {
    guard let media else {
      throw TeraRuntimeFailure.local(
        operation: "add.media.upload",
        code: "ios.add.media_unavailable",
        safeMessage: "Prepared photos are unavailable on this device."
      )
    }
    var status = initial
    guard let form = status.form else { return status }
    let opened = try await openedMedia(form.media)
    defer { opened.close() }
    try ensureCurrent(requestedGeneration)
    for mediaStatus in status.media where mediaStatus.stage != .verified {
      guard let persisted = form.media.first(where: { $0.remoteURL == mediaStatus.url }),
        let handle = opened.handles.first(where: {
          $0.media.opaqueReference == persisted.opaqueReference
        })
      else {
        throw TeraRuntimeFailure.local(
          operation: "add.media.upload",
          code: "ios.add.media_missing",
          safeMessage: "A prepared photo is unavailable."
        )
      }
      let intent = TeraBlossomUploadIntent(
        draftID: status.id,
        expectedRevision: status.revision,
        media: handle
      )
      let job = try await runtimeClient.prepareAddMediaBackground(input: intent)
      try accept(job.draft, generation: requestedGeneration)
      let receipt = try await media.uploadInBackground(job: job, media: persisted)
      try ensureCurrent(requestedGeneration)
      status = try await completeBackgroundUpload(receipt, handle: handle, media: media)
      try await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: true)
      try accept(status, generation: requestedGeneration)
      await refreshBlossomSnapshot()
      try ensureCurrent(requestedGeneration)
    }
    return status
  }

  private func completeBackgroundUpload(
    _ receipt: TeraAddBackgroundUploadReceipt,
    handle: TeraPreparedMediaHandle,
    media: any TeraAddMediaHandling
  ) async throws -> TeraDraftStatus {
    do {
      return try await runtimeClient.completeAddMediaBackground(
        input: TeraNativeUploadCompletion(
          draftID: receipt.draftID,
          expectedRevision: receipt.expectedRevision,
          media: handle,
          statusCode: receipt.statusCode,
          responseMediaType: receipt.mediaType,
          responseContentEncoding: receipt.contentEncoding,
          responseBody: receipt.body
        )
      )
    } catch is CancellationError {
      // Rust completion may already be durable. Leave the receipt awaiting
      // verification so relaunch can reconcile the unknown outcome.
      throw CancellationError()
    } catch {
      if TeraAddPresentation.failure(for: error)?.code == "ios.runtime.cancelled" {
        // The bounded runtime client cannot prove whether a cancelled FFI
        // completion became durable. Preserve the receipt for reconciliation.
        throw CancellationError()
      }
      try? await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: false)
      throw error
    }
  }

  private func openedMedia(_ values: [TeraPreparedMedia]? = nil) async throws
    -> TeraOpenedMedia
  {
    let values = values ?? form.media
    guard !values.isEmpty else { return TeraOpenedMedia(handles: [], files: []) }
    guard let media else {
      throw TeraRuntimeFailure.local(
        operation: "add.media.open",
        code: "ios.add.media_unavailable",
        safeMessage: "Prepared photos are unavailable on this device."
      )
    }
    return try await media.open(values)
  }

  private func reloadDrafts() async {
    let requestedGeneration = generation
    draftsGeneration = draftsGeneration.invalidated()
    let request = draftsGeneration
    do {
      let loaded = try await runtimeClient.draftHeads(limit: 100)
      guard isCurrent(requestedGeneration), request == draftsGeneration else { return }
      drafts = TeraAddPresentation.sorted(loaded)
      if let activeDraft, let current = loaded.first(where: { $0.id == activeDraft.id }),
         current.revision >= activeDraft.revision
      {
        self.activeDraft = current
      }
    } catch {
      guard isCurrent(requestedGeneration), request == draftsGeneration else { return }
      message = TeraAddPresentation.message(for: error)
    }
  }

  private func loadMediaSupport() async throws -> TeraAddMediaSupport {
    guard blossomConfiguration != nil, let media else { return .unavailable }
    return try await media.support()
  }

  @discardableResult
  private func refreshBlossomSnapshot() async -> Bool {
    let requestedGeneration = generation
    blossomGeneration = blossomGeneration.invalidated()
    let request = blossomGeneration
    let snapshot = try? await runtimeClient.snapshot()
    guard isCurrent(requestedGeneration), request == blossomGeneration else { return false }
    if let snapshot, configuration == TeraPresentationConfiguration(snapshot: snapshot) {
      blossomConfiguration = snapshot.blossomConfiguration
      blossomEvidence = snapshot.blossomEvidence
    }
    return true
  }

  private func startObservation() {
    observation.start(
      client: runtimeClient, capacity: 16, delay: observationDelay,
      state: { [weak self] in self?.observationState = $0 },
      change: { [weak self] change in
        guard let self else { return }
        let requested = generation
        if change.kind == .drafts || change.kind == .media {
          await reloadDrafts()
        }
        guard isCurrent(requested) else { return }
        if change.kind == .media || change.kind == .settings {
          await refreshBlossomSnapshot()
        }
      }
    )
  }

  private func isCurrent(_ requested: TeraSessionGeneration) -> Bool {
    requested == generation && generation.isActive && !Task.isCancelled
  }

  private func ensureCurrent(_ requested: TeraSessionGeneration) throws {
    guard isCurrent(requested) else { throw CancellationError() }
  }

  private func accept(
    _ status: TeraDraftStatus, generation requested: TeraSessionGeneration
  ) throws {
    try ensureCurrent(requested)
    draftsGeneration = draftsGeneration.invalidated()
    activeDraft = status
    if let form = status.form {
      self.form = form
    }
    drafts.removeAll(where: { $0.id == status.id })
    drafts.append(status)
    drafts = TeraAddPresentation.sorted(drafts)
  }

  private func accept(
    _ status: TeraRevisionStatus, generation requested: TeraSessionGeneration
  ) throws {
    try ensureCurrent(requested)
    revisionOperationID = status.operationID
    try accept(status.replacement, generation: requested)
    if let retraction = status.retraction {
      drafts.removeAll(where: { $0.id == retraction.id })
      drafts.append(retraction)
      drafts = TeraAddPresentation.sorted(drafts)
    }
  }

  private func perform(_ operation: @escaping (TeraSessionGeneration) async throws -> Void) async {
    guard operationTask == nil, !Task.isCancelled else { return }
    generation = generation.invalidated()
    let requestedGeneration = generation
    operationGeneration = requestedGeneration
    isWorking = true
    message = nil
    lastFailureCode = nil
    let task = Task { @MainActor [weak self] in
      guard let self, !Task.isCancelled else { return }
      await execute(operation, generation: requestedGeneration)
    }
    operationTask = task
    await task.value
  }

  private func execute(
    _ operation: @escaping (TeraSessionGeneration) async throws -> Void,
    generation requestedGeneration: TeraSessionGeneration
  ) async {
    defer {
      if operationGeneration == requestedGeneration {
        isWorking = false
        operationGeneration = nil
        operationTask = nil
      }
    }
    do {
      try ensureCurrent(requestedGeneration)
      try await operation(requestedGeneration)
    } catch is CancellationError {
      if isCurrent(requestedGeneration) {
        message = TeraUserMessages.text(.operationCancelled)
      }
    } catch {
      if isCurrent(requestedGeneration) {
        await refreshBlossomSnapshot()
        guard isCurrent(requestedGeneration) else { return }
        message = TeraAddPresentation.message(for: error)
        lastFailureCode = TeraAddPresentation.failure(for: error)?.code
      }
    }
  }
}
