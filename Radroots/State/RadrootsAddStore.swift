import Foundation

enum RadrootsAddLoadState: Sendable, Equatable {
  case idle
  case loading
  case ready
  case failed(String)
}

@MainActor
final class RadrootsAddStore: ObservableObject {
  @Published private(set) var schemas: [RadrootsAddSchema] = []
  @Published private(set) var drafts: [RadrootsDraftStatus] = []
  @Published private(set) var activeDraft: RadrootsDraftStatus?
  @Published private(set) var form: RadrootsAddForm
  @Published private(set) var state: RadrootsAddLoadState = .idle
  @Published private(set) var mediaSupport: RadrootsAddMediaSupport = .unavailable
  @Published private(set) var blossomConfiguration: RadrootsBlossomConfigurationStatus?
  @Published private(set) var blossomEvidence: RadrootsBlossomEvidence?
  @Published private(set) var isCheckingBlossom = false
  @Published private(set) var isWorking = false
  @Published private(set) var message: String?
  @Published private(set) var lastFailureCode: String?
  @Published private(set) var observationState: RadrootsRuntimeObservationState = .inactive

  private let runtimeClient: RadrootsRuntimeClient
  private let media: (any RadrootsAddMediaHandling)?
  private let observationDelay: @Sendable (UInt32) async throws -> Void
  private let identifier: @Sendable () -> String
  private let nowUnixSeconds: @Sendable () -> UInt64
  private let nowUnixMilliseconds: @Sendable () -> UInt64
  private var generation: UInt64 = 0
  private var operationGeneration: UInt64?
  private var operationTask: Task<Void, Never>?
  private var observationTask: Task<Void, Never>?
  private var isStarted = false
  private var observationGeneration: UInt64 = 0
  private var revisionTarget: RadrootsRevisionTarget?
  private var revisionOperationID: String?
  private var activePublicKey: String?

  init(
    runtimeClient: RadrootsRuntimeClient,
    media: (any RadrootsAddMediaHandling)? = nil,
    initialType: RadrootsAddCommandType = .createUpdate,
    identifier: @escaping @Sendable () -> String = {
      UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()
    },
    nowUnixSeconds: @escaping @Sendable () -> UInt64 = {
      UInt64(Date().timeIntervalSince1970)
    },
    nowUnixMilliseconds: @escaping @Sendable () -> UInt64 = {
      UInt64(Date().timeIntervalSince1970 * 1_000)
    },
    observationDelay: @escaping @Sendable (UInt32) async throws -> Void =
      RadrootsRuntimeObservationBackoff.sleep
  ) {
    self.runtimeClient = runtimeClient
    self.media = media
    self.identifier = identifier
    self.nowUnixSeconds = nowUnixSeconds
    self.nowUnixMilliseconds = nowUnixMilliseconds
    self.observationDelay = observationDelay
    form = Self.newForm(
      type: initialType,
      identifier: identifier,
      nowUnixSeconds: nowUnixSeconds
    )
  }

  deinit {
    observationTask?.cancel()
  }

  var selectedSchema: RadrootsAddSchema? {
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

  func configure(snapshot: RadrootsRuntimeSnapshot) {
    activePublicKey = snapshot.identity.publicKeyHex
    blossomConfiguration = snapshot.blossomConfiguration
    blossomEvidence = snapshot.blossomEvidence
  }

  func start() async {
    guard !isStarted else { return }
    isStarted = true
    startObservation()
    guard operationGeneration == nil else { return }
    message = nil
    state = .loading
    generation &+= 1
    let requestedGeneration = generation
    do {
      async let schemaResult = runtimeClient.addSchemas()
      async let draftResult = runtimeClient.draftHeads(limit: 100)
      async let supportResult = loadMediaSupport()
      let (loadedSchemas, loadedDrafts, loadedSupport) = try await (
        schemaResult,
        draftResult,
        supportResult
      )
      guard requestedGeneration == generation else { return }
      try await media?.reconcileBackgroundUploads(drafts: loadedDrafts)
      try Task.checkCancellation()
      guard requestedGeneration == generation else { return }
      schemas = try RadrootsProductSurfaceContract.validate(schemas: loadedSchemas)
      drafts = Self.sorted(loadedDrafts)
      mediaSupport = loadedSupport
      state = .ready
    } catch {
      guard requestedGeneration == generation else { return }
      state = .failed(Self.message(for: error))
    }
  }

  func stop() {
    generation &+= 1
    operationGeneration = nil
    operationTask?.cancel()
    operationTask = nil
    isStarted = false
    observationGeneration &+= 1
    observationTask?.cancel()
    observationTask = nil
    observationState = .stopped
    isWorking = false
  }

  func suspend() {
    isStarted = false
    observationGeneration &+= 1
    observationTask?.cancel()
    observationTask = nil
    observationState = .stopped
  }

  func selectType(_ type: RadrootsAddCommandType) {
    guard !isWorking, isFormEditable, form.commandType != type else { return }
    activeDraft = nil
    revisionTarget = nil
    revisionOperationID = nil
    form = Self.newForm(
      type: type,
      identifier: identifier,
      nowUnixSeconds: nowUnixSeconds
    )
    message = nil
  }

  func updateForm<Value>(_ keyPath: WritableKeyPath<RadrootsAddForm, Value>, _ value: Value) {
    guard isFormEditable else { return }
    form[keyPath: keyPath] = value
  }

  func newDraft(type: RadrootsAddCommandType? = nil) {
    guard !isWorking else { return }
    generation &+= 1
    activeDraft = nil
    revisionTarget = nil
    revisionOperationID = nil
    form = Self.newForm(
      type: type ?? form.commandType,
      identifier: identifier,
      nowUnixSeconds: nowUnixSeconds
    )
    message = nil
  }

  func reopen(_ draft: RadrootsDraftStatus) {
    guard !isWorking else { return }
    guard let snapshot = draft.form else {
      message = "This operation has no editable Add form."
      return
    }
    generation &+= 1
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
    await perform {
      let remaining = self.mediaLimit - self.form.media.count
      guard remaining > 0 else { return }
      let imported = try await media.importImages(limit: remaining)
      try Task.checkCancellation()
      self.form.media.append(contentsOf: imported.prefix(remaining))
      self.message = "Photo prepared. Add descriptive text before publishing."
    }
  }

  func checkPhotoService() async {
    guard !isCheckingBlossom else { return }
    guard blossomConfiguration != nil else {
      mediaSupport = .unavailable
      message = "No photo service is configured for the current network profile."
      return
    }
    isCheckingBlossom = true
    defer { isCheckingBlossom = false }
    do {
      blossomEvidence = try await runtimeClient.probeBlossom()
      mediaSupport = try await loadMediaSupport()
      message = "Photo service is reachable."
    } catch {
      await refreshBlossomSnapshot()
      mediaSupport = .unavailable
      message = Self.message(for: error)
    }
  }

  func capturePhoto() async {
    guard canAddMedia, let media else {
      message = "Camera intake is unavailable."
      return
    }
    await perform {
      let captured = try await media.captureImage()
      try Task.checkCancellation()
      guard self.form.media.count < self.mediaLimit else { return }
      self.form.media.append(captured)
      self.message = "Photo prepared. Add descriptive text before publishing."
    }
  }

  func removeMedia(id: String) {
    guard isFormEditable else { return }
    form.media.removeAll(where: { $0.id == id })
  }

  func updateMediaAlt(id: String, alt: String) {
    guard isFormEditable,
      let index = form.media.firstIndex(where: { $0.id == id })
    else { return }
    form.media[index].alt = alt
  }

  func save() async {
    await perform {
      _ = try await self.saveCurrentForm()
      self.message = "Draft saved on this device."
    }
  }

  func submit() async {
    await perform {
      var status: RadrootsDraftStatus =
        if let active = self.activeDraft, !active.state.isEditable {
          active
        } else {
          try await self.saveCurrentForm()
        }

      if !status.media.isEmpty,
        status.media.contains(where: { $0.stage != .verified })
      {
        status = try await self.uploadPendingMedia(status)
      }

      if status.isRevision {
        let revision = try await self.runtimeClient.advanceRevision(
          operationID: self.revisionOperationID ?? status.id
        )
        try Task.checkCancellation()
        self.accept(revision)
        self.message = revision.honestSummary
        return
      }

      if status.state.isEditable || status.state == .readyToSign {
        do {
          status = try await self.runtimeClient.queueAddIntent(
            id: status.id,
            expectedRevision: status.revision
          )
          self.accept(status)
        } catch {
          if Self.failure(for: error)?.code == "writable_relay_unavailable" {
            self.accept(status)
            self.message =
              status.media.isEmpty
              ? "Draft saved. Configure a writable relay to publish."
              : "Photo verified and draft saved. Configure a writable relay to publish."
            return
          }
          throw error
        }
      }

      do {
        if status.state.canAdvance {
          status = try await self.runtimeClient.advanceDraft(
            id: status.id,
            expectedRevision: status.revision
          )
          self.accept(status)
        }
        self.message = status.honestSummary
      } catch {
        // Queueing is the commit point. A later retry must reuse this immutable snapshot.
        self.message = "Saved for retry. \(Self.message(for: error))"
      }
    }
  }

  func retry(_ draft: RadrootsDraftStatus? = nil) async {
    guard let draft = draft ?? activeDraft else { return }
    await perform {
      var current = try await self.runtimeClient.draftStatus(id: draft.id)
      try Task.checkCancellation()
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
        try Task.checkCancellation()
        self.accept(revision)
        self.message = revision.honestSummary
        return
      }
      if current.state.canAdvance {
        current = try await self.runtimeClient.advanceDraft(
          id: current.id,
          expectedRevision: current.revision
        )
      }
      self.accept(current)
      self.message = current.honestSummary
    }
  }

  func cancel(_ draft: RadrootsDraftStatus? = nil) async {
    guard let draft = draft ?? activeDraft, draft.state.canCancel else { return }
    await perform {
      let current = try await self.runtimeClient.draftStatus(id: draft.id)
      try Task.checkCancellation()
      if current.isRevision {
        let cancelled = try await self.runtimeClient.cancelRevision(
          operationID: current.id
        )
        try Task.checkCancellation()
        self.accept(cancelled)
        self.message = cancelled.honestSummary
        return
      }
      let cancelled = try await self.runtimeClient.cancelAddIntent(
        id: current.id,
        expectedRevision: current.revision
      )
      self.accept(cancelled)
      self.message = "Local work was cancelled. Any already-published relay effect is preserved."
    }
  }

  func retractAndRevise(_ card: RadrootsTodayCard) async {
    await perform {
      guard let publicKey = self.activePublicKey, publicKey == card.authorPublicKey else {
        throw RadrootsRuntimeFailure.local(
          operation: "add.revise",
          code: "ios.add.revision_not_authorized",
          safeMessage: "Only your own post can be revised."
        )
      }
      guard let operationID = card.localOperationID else {
        throw RadrootsRuntimeFailure.local(
          operation: "add.revise",
          code: "ios.add.revision_source_unavailable",
          safeMessage: "This post cannot be revised losslessly on this device."
        )
      }
      let source = try await self.runtimeClient.draftStatus(id: operationID)
      try Task.checkCancellation()
      guard let sourceForm = source.form else {
        throw RadrootsRuntimeFailure.local(
          operation: "add.revise",
          code: "ios.add.revision_form_unavailable",
          safeMessage: "The original Add form is unavailable on this device."
        )
      }
      self.revisionTarget = RadrootsRevisionTarget(
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

  func retract(_ card: RadrootsTodayCard) async {
    await perform {
      guard let publicKey = self.activePublicKey, publicKey == card.authorPublicKey else {
        throw RadrootsRuntimeFailure.local(
          operation: "add.retract",
          code: "ios.add.retraction_not_authorized",
          safeMessage: "Only your own post can be retracted."
        )
      }
      guard let targetKind = card.retractionTargetKind else {
        throw RadrootsRuntimeFailure.local(
          operation: "add.retract",
          code: "ios.add.retraction_target_invalid",
          safeMessage: "This post cannot be retracted safely."
        )
      }
      let draftID = self.identifier()
      guard Self.isValidIdentifier(draftID) else {
        throw RadrootsRuntimeFailure.local(
          operation: "add.retract",
          code: "ios.add.identifier_invalid",
          safeMessage: "The local operation identifier is invalid."
        )
      }
      var status = try await self.runtimeClient.saveRetractionDraft(
        id: draftID,
        input: RadrootsRetractionDraftInput(
          commandType: card.type.addCommandType,
          targetCardID: card.id,
          targetEventID: card.sourceEventID,
          targetKind: targetKind,
          targetAddress: card.sourceAddress,
          reason: "Removed by author."
        ),
        authoredAtUnixSeconds: self.nowUnixSeconds(),
        persistedAtUnixMilliseconds: self.nowUnixMilliseconds()
      )
      self.accept(status)
      status = try await self.runtimeClient.queueAddIntent(
        id: status.id,
        expectedRevision: status.revision
      )
      self.accept(status)
      if status.state.canAdvance {
        status = try await self.runtimeClient.advanceDraft(
          id: status.id,
          expectedRevision: status.revision
        )
        self.accept(status)
      }
      self.message = status.honestSummary
    }
  }

  private func saveCurrentForm() async throws -> RadrootsDraftStatus {
    guard isFormEditable else {
      throw RadrootsRuntimeFailure.local(
        operation: "add.save",
        code: "ios.add.form_frozen",
        safeMessage: "Submitted drafts cannot be changed. Create a revised copy instead."
      )
    }
    let opened = try await openedMedia()
    defer { opened.close() }
    let input = RadrootsAddRuntimeInput(form: form, media: opened.handles)
    if let target = revisionTarget {
      let revision = try await runtimeClient.saveRevisionIntent(
        target: target,
        replacement: input
      )
      try Task.checkCancellation()
      revisionOperationID = revision.operationID
      self.revisionTarget = nil
      accept(revision)
      return revision.replacement
    }
    let status = try await runtimeClient.saveAddIntent(
      input: input,
      existingDraftID: activeDraft?.isRevision == true ? nil : activeDraft?.id,
      expectedRevision: activeDraft?.isRevision == true ? nil : activeDraft?.revision
    )
    try Task.checkCancellation()
    accept(status)
    return status
  }

  private func uploadPendingMedia(_ initial: RadrootsDraftStatus) async throws
    -> RadrootsDraftStatus
  {
    guard let media else {
      throw RadrootsRuntimeFailure.local(
        operation: "add.media.upload",
        code: "ios.add.media_unavailable",
        safeMessage: "Prepared photos are unavailable on this device."
      )
    }
    var status = initial
    guard let form = status.form else { return status }
    let opened = try await openedMedia(form.media)
    defer { opened.close() }
    for mediaStatus in status.media where mediaStatus.stage != .verified {
      guard let persisted = form.media.first(where: { $0.remoteURL == mediaStatus.url }),
        let handle = opened.handles.first(where: {
          $0.media.opaqueReference == persisted.opaqueReference
        })
      else {
        throw RadrootsRuntimeFailure.local(
          operation: "add.media.upload",
          code: "ios.add.media_missing",
          safeMessage: "A prepared photo is unavailable."
        )
      }
      let intent = RadrootsBlossomUploadIntent(
        draftID: status.id,
        expectedRevision: status.revision,
        media: handle
      )
      let job = try await runtimeClient.prepareAddMediaBackground(input: intent)
      try Task.checkCancellation()
      accept(job.draft)
      let receipt = try await media.uploadInBackground(job: job, media: persisted)
      try Task.checkCancellation()
      do {
        status = try await runtimeClient.completeAddMediaBackground(
          input: RadrootsNativeUploadCompletion(
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
        if Self.failure(for: error)?.code == "ios.runtime.cancelled" {
          // The bounded runtime client cannot prove whether a cancelled FFI
          // completion became durable. Preserve the receipt for reconciliation.
          throw CancellationError()
        }
        try? await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: false)
        throw error
      }
      try await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: true)
      try Task.checkCancellation()
      accept(status)
      await refreshBlossomSnapshot()
    }
    return status
  }

  private func openedMedia(_ values: [RadrootsPreparedMedia]? = nil) async throws
    -> RadrootsOpenedMedia
  {
    let values = values ?? form.media
    guard !values.isEmpty else { return RadrootsOpenedMedia(handles: [], files: []) }
    guard let media else {
      throw RadrootsRuntimeFailure.local(
        operation: "add.media.open",
        code: "ios.add.media_unavailable",
        safeMessage: "Prepared photos are unavailable on this device."
      )
    }
    return try await media.open(values)
  }

  private func reloadDrafts() async {
    do {
      let loaded = try await runtimeClient.draftHeads(limit: 100)
      drafts = Self.sorted(loaded)
      if let activeDraft,
        let current = loaded.first(where: { $0.id == activeDraft.id })
      {
        self.activeDraft = current
      }
    } catch {
      message = Self.message(for: error)
    }
  }

  private func loadMediaSupport() async throws -> RadrootsAddMediaSupport {
    guard blossomConfiguration != nil, let media else { return .unavailable }
    return try await media.support()
  }

  private func refreshBlossomSnapshot() async {
    guard let snapshot = try? await runtimeClient.snapshot() else { return }
    blossomConfiguration = snapshot.blossomConfiguration
    blossomEvidence = snapshot.blossomEvidence
  }

  private func startObservation() {
    observationGeneration &+= 1
    let requestedGeneration = observationGeneration
    observationTask = Task { [weak self] in
      await self?.observe(generation: requestedGeneration)
    }
  }

  private func observe(generation: UInt64) async {
    var attempt: UInt32 = 0
    while isStarted, observationGeneration == generation, !Task.isCancelled {
      observationState = .subscribing(attempt: attempt &+ 1)
      var failureMessage = "Runtime observation ended unexpectedly."
      do {
        let changes = try await runtimeClient.changes(bufferCapacity: 16)
        observationState = .active
        for await change in changes {
          guard !Task.isCancelled else { break }
          attempt = 0
          if change.kind == .drafts || change.kind == .media {
            await reloadDrafts()
          }
          if change.kind == .media || change.kind == .settings {
            await refreshBlossomSnapshot()
          }
        }
      } catch {
        failureMessage =
          (error as? LocalizedError)?.errorDescription
          ?? "Runtime observation is temporarily unavailable."
      }
      guard isStarted, observationGeneration == generation, !Task.isCancelled else { break }
      attempt = attempt == .max ? .max : attempt + 1
      observationState = .retrying(attempt: attempt, message: failureMessage)
      do {
        try await observationDelay(attempt)
      } catch {
        break
      }
    }
    guard observationGeneration == generation else { return }
    observationTask = nil
    isStarted = false
    if case .retrying = observationState {
      return
    }
    observationState = .stopped
  }

  private func accept(_ status: RadrootsDraftStatus) {
    guard operationGeneration == generation else { return }
    activeDraft = status
    if let form = status.form {
      self.form = form
    }
    drafts.removeAll(where: { $0.id == status.id })
    drafts.append(status)
    drafts = Self.sorted(drafts)
  }

  private func accept(_ status: RadrootsRevisionStatus) {
    guard operationGeneration == generation else { return }
    revisionOperationID = status.operationID
    accept(status.replacement)
    if let retraction = status.retraction {
      drafts.removeAll(where: { $0.id == retraction.id })
      drafts.append(retraction)
      drafts = Self.sorted(drafts)
    }
  }

  private func perform(_ operation: @escaping () async throws -> Void) async {
    guard operationTask == nil else { return }
    generation &+= 1
    let requestedGeneration = generation
    operationGeneration = requestedGeneration
    isWorking = true
    message = nil
    lastFailureCode = nil
    let task = Task { @MainActor [weak self] in
      guard let self else { return }
      await self.execute(operation, generation: requestedGeneration)
    }
    operationTask = task
    await task.value
  }

  private func execute(
    _ operation: @escaping () async throws -> Void,
    generation requestedGeneration: UInt64
  ) async {
    do {
      try await operation()
    } catch is CancellationError {
      if requestedGeneration == generation {
        message = "The operation was cancelled."
      }
    } catch {
      if requestedGeneration == generation {
        await refreshBlossomSnapshot()
        message = Self.message(for: error)
        lastFailureCode = Self.failure(for: error)?.code
      }
    }
    if requestedGeneration == generation {
      isWorking = false
      operationGeneration = nil
      operationTask = nil
    }
  }

  private static func sorted(_ drafts: [RadrootsDraftStatus]) -> [RadrootsDraftStatus] {
    drafts.sorted {
      if $0.updatedAtUnixMilliseconds == $1.updatedAtUnixMilliseconds {
        return $0.id < $1.id
      }
      return $0.updatedAtUnixMilliseconds > $1.updatedAtUnixMilliseconds
    }
  }

  private static func newForm(
    type: RadrootsAddCommandType,
    identifier: @Sendable () -> String,
    nowUnixSeconds: @Sendable () -> UInt64
  ) -> RadrootsAddForm {
    var form = RadrootsAddForm.empty(type)
    if type == .createEvent || type == .createFoodAvailability {
      let value = identifier()
      if isValidIdentifier(value) {
        form.identifier = value
      }
    }
    if type == .createEvent {
      let now = nowUnixSeconds()
      let start = now.addingReportingOverflow(3_600).overflow ? now : now + 3_600
      let end = start.addingReportingOverflow(3_600).overflow ? start : start + 3_600
      form.eventStartUnixSeconds = start
      form.eventEndUnixSeconds = end
      form.eventStartDate = eventDateFormatter.string(
        from: Date(timeIntervalSince1970: TimeInterval(start))
      )
      form.eventEndDate = eventDateFormatter.string(
        from: Date(timeIntervalSince1970: TimeInterval(end))
      )
    }
    return form
  }

  private static func isValidIdentifier(_ value: String) -> Bool {
    value.utf8.count == 32
      && value.utf8.allSatisfy { byte in
        (byte >= 0x30 && byte <= 0x39) || (byte >= 0x61 && byte <= 0x66)
      }
  }

  private static let eventDateFormatter: DateFormatter = {
    let formatter = DateFormatter()
    formatter.calendar = Calendar(identifier: .gregorian)
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.timeZone = TimeZone(secondsFromGMT: 0)
    formatter.dateFormat = "yyyy-MM-dd"
    return formatter
  }()

  private static func message(for error: Error) -> String {
    if let failure = failure(for: error) {
      return failure.safeMessage
    }
    return (error as? LocalizedError)?.errorDescription
      ?? "The Add operation could not be completed."
  }

  private static func failure(for error: Error) -> RadrootsRuntimeFailure? {
    if case RadrootsRuntimeClientError.add(let failure) = error {
      return failure
    }
    return error as? RadrootsRuntimeFailure
  }

}
