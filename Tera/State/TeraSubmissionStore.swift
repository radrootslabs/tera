import Foundation

/// One explicit native waiter. The runtime retains admission for a late FFI
/// callback after a waiter deadline; Rust owns durable receipts and effects.
/// Editing changes never invalidate this worker or replace its captured request.
@MainActor
final class TeraSubmissionStore: ObservableObject {
  @Published private(set) var request: TeraSubmissionRequest?
  @Published private(set) var status: TeraSubmissionStatus?
  @Published private(set) var isWorking = false
  @Published private(set) var message: String?
  @Published private(set) var failureCode: String?
  let inventory: TeraSubmissionInventory
  var changed: () -> Void = {}
  private let client: TeraRuntimeClient
  private let composer: TeraComposerAutosave
  private let media: (any TeraAddMediaHandling)?
  private var capture: TeraComposerCapture?
  private var scope: TeraComposerScope?
  private var context: TeraLocalNetwork?
  private var generation = TeraSessionGeneration.initial
  private var worker: Task<Void, Never>?
  private var paused = false
  private let stopControl = TeraSubmissionStopControl()

  init(client: TeraRuntimeClient, composer: TeraComposerAutosave, media: (any TeraAddMediaHandling)?) {
    self.client = client
    self.composer = composer
    self.media = media
    inventory = TeraSubmissionInventory(client: client)
  }

  deinit { worker?.cancel() }

  var hasAction: Bool {
    request != nil || capture != nil
  }

  func configure(scope: TeraComposerScope, context: TeraLocalNetwork) {
    self.context = context
    guard self.scope != scope else { return }
    stop()
    self.scope = scope
    capture = nil
    request = nil
    status = nil
    stopControl.reset()
    message = nil
    failureCode = nil
    inventory.configure(scope: scope)
    changed()
  }

  func start() {
    paused = false
    inventory.start()
  }

  func refreshSelected() async {
    if request != nil, worker == nil {
      await run(advancing: false)
    }
  }

  /// Explicit continuation uses only the retained original request/capture.
  /// The runtime remains authoritative for every effect and retry admission.
  func continueSelected() async {
    guard hasAction, status?.canOfferContinuation != false else { return }
    await run(advancing: true)
  }

  func stop() {
    generation = generation.invalidated()
    paused = true
    worker?.cancel()
    inventory.stop()
    // Keep this waiter until it returns. Runtime admission separately remains
    // occupied until an abandoned FFI callback has actually completed.
  }

  func stopWaiting() {
    worker?.cancel()
    message = "Waiting stopped. The original request is retained; remote effects may still complete."
    changed()
  }

  func requestStop() async {
    guard !paused else { return }
    stopControl.request()
    let requested = generation
    message = "Stop is pending confirmation. The original request and its effects are retained."
    changed()
    guard let request else { return }
    do {
      let stopped = try await client.requestSubmissionStop(request: request)
      try accept(stopped, generation: requested)
      message = status?.summary
    } catch {
      guard generation == requested, !paused, self.request == request else { return }
      message = "Stop is not yet confirmed. Reconcile the original request to retain the stop."
    }
    changed()
  }

  func submit(form: TeraAddForm) async {
    guard !paused, !Task.isCancelled else { return }
    if let worker {
      await worker.value; return
    }
    do {
      if !hasAction {
        capture = try composer.beginSubmissionCapture(TeraComposerForm(editing: form))
      }
    } catch {
      message = TeraAddPresentation.message(for: error)
      changed()
      return
    }
    await run(advancing: true)
  }

  /// Viewing saved work does not change the editing buffer or begin an effect.
  func select(_ summary: TeraSubmissionSummary) async {
    guard worker == nil, !paused, summary.request.scope == scope else { return }
    guard capture == nil else {
      message = "Reconcile the original submission or choose New before selecting another operation."
      changed()
      return
    }
    request = summary.request
    status = nil
    stopControl.reset()
    await run(advancing: false)
  }

  /// New is explicit. Preserve the old captured source in its own composer row
  /// before the editing interlock saves newer input under a fresh composer ID.
  func preserveForReplacement(currentForm: () -> TeraAddForm) async throws {
    guard worker == nil else {
      throw TeraRuntimeFailure.local(operation: "submission.new", code: "operation_in_progress",
                                     safeMessage: "The original submission is still returning. Keep editing and retry New when it finishes.")
    }
    if capture != nil {
      let requested = generation
      if let request {
        _ = try await client.recoverSubmission(request: request)
      }
      try ensureCurrent(requested)
      composer.reset(scope: scope)
      composer.change(TeraComposerForm(editing: currentForm()))
      capture = nil
    }
  }

  func newAction() {
    guard worker == nil else { return }
    capture = nil
    request = nil
    status = nil
    stopControl.reset()
    message = nil
    failureCode = nil
    changed()
    inventory.start()
  }

  private func run(advancing: Bool) async {
    guard worker == nil, !paused else { return }
    let requested = generation
    isWorking = true
    message = nil
    failureCode = nil
    changed()
    let task = Task { @MainActor [weak self] in
      guard let self else { return }
      await execute(advancing: advancing, generation: requested)
      worker = nil
      isWorking = false
      changed()
    }
    worker = task
    // Cancelling the view's waiter does not cancel durable work. Stop waiting
    // is an explicit host action; both paths retain the same request.
    await task.value
  }

  private func execute(advancing: Bool, generation requested: TeraSessionGeneration) async {
    do {
      try ensureCurrent(requested)
      let current = try await resolve(advancing: advancing, generation: requested)
      guard var current else {
        message = "The original submission is reserved. Retry it to finish local preparation."
        changed()
        return
      }
      try accept(current, generation: requested)
      if stopControl.requested, !current.delivery.isStopped {
        let stopped = try await client.requestSubmissionStop(request: current.request)
        try accept(stopped, generation: requested)
      }
      current = status ?? current
      try await reconcileLocal(current, generation: requested)
      if advancing {
        let effects = TeraSubmissionEffects(client: client, media: media,
                                            ensure: { try self.ensureCurrent(requested) },
                                            accept: { try self.accept($0, generation: requested) },
                                            mayStart: { !self.stopControl.requested }, stopControl: stopControl)
        try await effects.advance(current)
      } else {
        try await reconcileBackground(current, generation: requested)
      }
      try ensureCurrent(requested)
      if let status {
        try await reconcileLocal(status, generation: requested)
      }
      message = status?.summary
    } catch {
      guard generation == requested, !paused else { return }
      if let request, !Task.isCancelled {
        // Read after every uncertain result. Never infer absence from a read
        // error, open newer form media, or create another operation on retry.
        if let recovered = try? await client.recoverSubmission(request: request) {
          try? accept(recovered, generation: requested)
          try? await reconcileLocal(recovered, generation: requested)
        }
      }
      guard generation == requested, !paused else { return }
      failureCode = TeraAddPresentation.failure(for: error)?.code
      message = Task.isCancelled
        ? "Waiting stopped. Retry the original request to reconcile its outcome."
        : "Original request retained. \(TeraAddPresentation.message(for: error))"
    }
    guard generation == requested, !paused else { return }
    changed()
    inventory.start()
  }

  private func reconcileLocal(_ current: TeraSubmissionStatus, generation requested: TeraSessionGeneration) async throws {
    try ensureCurrent(requested)
    guard current.settlement.signed > 0 else { return }
    guard let context, context.id == current.request.scope.localNetworkID else { throw TeraComposerAcknowledgment.unconfirmed }
    let reconciled = try await client.reconcileSubmissionLocal(request: current.request, context: context)
    try accept(reconciled, generation: requested)
  }

  private func accept(_ value: TeraSubmissionStatus, generation requested: TeraSessionGeneration) throws {
    try ensureCurrent(requested)
    guard value.request == request, value.request.scope == scope,
          status == nil || (status?.operationID == value.operationID && status?.intentID == value.intentID
            && status?.captured == value.captured)
    else {
      throw TeraComposerAcknowledgment.unconfirmed
    }
    if let status, value.revision < status.revision || !value.delivery.follows(status.delivery)
      || !value.targetDetails.follows(status.targetDetails)
      || value.settlement.signed < status.settlement.signed || value.settlement.admitted < status.settlement.admitted {
        return
      }
    status = value
    if let capture {
      composer.releaseSubmissionCapture(capture)
      self.capture = nil
    }
    changed()
  }

  private func ensureCurrent(_ requested: TeraSessionGeneration) throws {
    guard requested == generation, generation.isActive, !paused, !Task.isCancelled else { throw CancellationError() }
  }
}

private extension TeraSubmissionStore {
  private func resolve(advancing: Bool, generation requested: TeraSessionGeneration) async throws -> TeraSubmissionStatus? {
      if request == nil {
        guard let capture else { throw TeraComposerAcknowledgment.unconfirmed }
        let saved = try await composer.saveSubmissionCapture(capture)
        try ensureCurrent(requested)
        let commandID = try await client.reserveSubmissionID()
        try ensureCurrent(requested)
        request = TeraSubmissionRequest(commandID: commandID, scope: saved.scope,
                                        composerID: saved.id, expectedRevision: saved.revision)
        changed()
      }
      guard let request else { throw TeraComposerAcknowledgment.unconfirmed }
      var current = try await client.recoverSubmission(request: request)
      try ensureCurrent(requested)
      if current == nil, advancing {
        let reservation = try await client.reserveSubmission(request: request)
        try ensureCurrent(requested)
        if let capture {
          guard reservation.commandID == request.commandID,
                reservation.captured.scope == request.scope,
                reservation.captured.id == request.composerID,
                reservation.captured.revision == request.expectedRevision
          else {
            throw TeraComposerAcknowledgment.unconfirmed
          }
          try composer.continueEditing(after: capture, reserved: reservation.captured)
          self.capture = nil
        }
        let opened = try await TeraOpenedMedia.open(reservation.captured.form.editingValue.media, using: media)
        defer { opened.close() }
        try ensureCurrent(requested)
        current = try await client.prepareSubmission(request: request, media: opened.handles)
      }
      try ensureCurrent(requested)
      return current
  }

  private func reconcileBackground(_ current: TeraSubmissionStatus, generation requested: TeraSessionGeneration) async throws {
    if current.delivery.isStopped {
      try await TeraSubmissionEffects(client: client, media: media,
                                      ensure: { try self.ensureCurrent(requested) },
                                      accept: { try self.accept($0, generation: requested) }).reconcileStopped(current)
    } else {
      try await media?.reconcileBackgroundSubmissions([current], client: client)
    }
  }
}
