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
    update: TeraTodayProjectionUpdate
  ) async throws -> TeraTodayRefreshReceipt
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

private enum TeraRuntimeBoundedOutcome<Value: Sendable>: Sendable {
  case completed(Result<Value, TeraRuntimeFailure>)
  case timedOut
  case cancelled
}

private final class TeraRuntimeBoundedTask<Value: Sendable>: @unchecked Sendable {
  typealias Outcome = TeraRuntimeBoundedOutcome<Value>

  private final class State: @unchecked Sendable {
    private let lock = NSLock()
    private var outcome: Outcome?
    private var continuations: [UUID: CheckedContinuation<Outcome, Never>] = [:]
    private var cancelledWaiters: Set<UUID> = []
    private var operationTask: Task<Void, Never>?
    private var timeoutTask: Task<Void, Never>?

    func install(operationTask: Task<Void, Never>, timeoutTask: Task<Void, Never>) {
      let shouldCancel = lock.withLock { () -> Bool in
        guard outcome == nil else { return true }
        self.operationTask = operationTask
        self.timeoutTask = timeoutTask
        return false
      }
      if shouldCancel {
        timeoutTask.cancel()
      }
    }

    func value(cancelsOperationWhenWaiterCancelled: Bool) async -> Outcome {
      let waiterID = UUID()
      return await withTaskCancellationHandler {
        await withCheckedContinuation { requestedContinuation in
          let immediate = lock.withLock { () -> Outcome? in
            if let outcome {
              return outcome
            }
            if cancelledWaiters.remove(waiterID) != nil {
              return .cancelled
            }
            continuations[waiterID] = requestedContinuation
            return nil
          }
          if let immediate {
            requestedContinuation.resume(returning: immediate)
          }
        }
      } onCancel: {
        if cancelsOperationWhenWaiterCancelled {
          cancel()
        } else {
          cancelWaiter(waiterID)
        }
      }
    }

    func cancel() {
      guard resolve(.cancelled) else { return }
      let tasks = lock.withLock { (operationTask, timeoutTask) }
      tasks.0?.cancel()
      tasks.1?.cancel()
    }

    func expire() {
      guard resolve(.timedOut) else { return }
      let taskToCancel: Task<Void, Never>? = lock.withLock { self.operationTask }
      taskToCancel?.cancel()
    }

    func finishOperation() {
      lock.withLock { operationTask = nil }
    }

    private func cancelWaiter(_ waiterID: UUID) {
      let continuation = lock.withLock { () -> CheckedContinuation<Outcome, Never>? in
        guard outcome == nil else { return nil }
        guard let continuation = continuations.removeValue(forKey: waiterID) else {
          cancelledWaiters.insert(waiterID)
          return nil
        }
        return continuation
      }
      continuation?.resume(returning: .cancelled)
    }

    @discardableResult
    func resolve(_ requestedOutcome: Outcome) -> Bool {
      var pendingContinuations: [CheckedContinuation<Outcome, Never>] = []
      var timeoutToCancel: Task<Void, Never>?
      let accepted = lock.withLock { () -> Bool in
        guard outcome == nil else { return false }
        outcome = requestedOutcome
        pendingContinuations = Array(continuations.values)
        continuations.removeAll(keepingCapacity: false)
        cancelledWaiters.removeAll(keepingCapacity: false)
        timeoutToCancel = timeoutTask
        return true
      }
      if accepted {
        timeoutToCancel?.cancel()
        for continuation in pendingContinuations {
          continuation.resume(returning: requestedOutcome)
        }
      }
      return accepted
    }
  }

  private let state: State

  init(
    deadlineNanoseconds: UInt64,
    operation: @escaping @Sendable () async -> Result<Value, TeraRuntimeFailure>,
    onAbandonedResult:
    @escaping @Sendable (
      Result<Value, TeraRuntimeFailure>
    ) async -> Void = { _ in }
  ) {
    let state = State()
    self.state = state
    let operationTask = Task { [state] in
      let result = await operation()
      if !state.resolve(.completed(result)) {
        await onAbandonedResult(result)
      }
      state.finishOperation()
    }

    let timeoutTask = Task { [state] in
      do {
        try await Task.sleep(nanoseconds: deadlineNanoseconds)
      } catch {
        return
      }
      state.expire()
    }
    state.install(operationTask: operationTask, timeoutTask: timeoutTask)
  }

  func value(cancelsOperationWhenWaiterCancelled: Bool = true) async -> Outcome {
    await state.value(
      cancelsOperationWhenWaiterCancelled: cancelsOperationWhenWaiterCancelled
    )
  }

  func cancel() {
    state.cancel()
  }
}

actor TeraRuntimeClient {
  private struct StartupOperation: Sendable {
    let identity: TeraRuntimeOperationIdentity
    let configuration: TeraRuntimeLaunchConfiguration
    let task: TeraRuntimeBoundedTask<TeraRuntimeBackendStart>
  }

  private struct ShutdownOperation: Sendable {
    let identity: TeraRuntimeOperationIdentity
    let backend: (any TeraRuntimeBackend)?
    let task: TeraRuntimeBoundedTask<TeraRuntimeShutdownReceipt>
  }

  private struct ActiveOperation: Sendable {
    let identity: TeraRuntimeOperationIdentity
    let cancel: @Sendable () -> Void
  }

  private struct Subscription {
    let generation: TeraSessionGeneration
    let continuation: AsyncStream<TeraRuntimeChange>.Continuation
    var token: (any TeraRuntimeSubscriptionToken)?
  }

  private let factory: TeraRuntimeBackendFactory
  private let deadlines: TeraRuntimeDeadlinePolicy
  private var generation = TeraSessionGeneration.initial
  private var operationSequence: UInt64 = 0
  private var lifecycleState: TeraRuntimeLifecycle = .stopped
  private var configuration: TeraRuntimeLaunchConfiguration?
  private var backend: (any TeraRuntimeBackend)?
  private var quarantinedBackend: (any TeraRuntimeBackend)?
  private var startupOperation: StartupOperation?
  private var shutdownOperation: ShutdownOperation?
  private var activeOperations: [UInt64: ActiveOperation] = [:]
  private var subscriptions: [UUID: Subscription] = [:]
  private var lateShutdownSuccesses: Set<TeraRuntimeOperationIdentity> = []

  init(
    factory: @escaping TeraRuntimeBackendFactory,
    deadlines: TeraRuntimeDeadlinePolicy = .production
  ) {
    self.factory = factory
    self.deadlines = deadlines
  }

  func lifecycle() -> TeraRuntimeLifecycle {
    lifecycleState
  }

  func start(
    configuration requestedConfiguration: TeraRuntimeLaunchConfiguration
  ) async throws -> TeraRuntimeSnapshot {
    try await start(configuration: requestedConfiguration, kind: .startup)
  }

  func reconfigure(
    configuration requestedConfiguration: TeraRuntimeLaunchConfiguration
  ) async throws -> TeraRuntimeSnapshot {
    try await start(configuration: requestedConfiguration, kind: .reconfiguration)
  }

  private func start(
    configuration requestedConfiguration: TeraRuntimeLaunchConfiguration,
    kind: TeraRuntimeOperationKind
  ) async throws -> TeraRuntimeSnapshot {
    if let shutdownOperation {
      _ = try await finishShutdown(shutdownOperation)
    }

    if quarantinedBackend != nil {
      _ = try await finishShutdown(beginShutdown())
    }

    if backend != nil,
       configuration == requestedConfiguration,
       case .running = lifecycleState
    {
      return try await snapshot()
    }

    if let startupOperation,
       startupOperation.configuration == requestedConfiguration
    {
      return try await finishStartup(startupOperation)
    }

    if startupOperation != nil || backend != nil {
      let operation = beginShutdown()
      _ = try await finishShutdown(operation)
    }

    generation = try generation.next()
    let operationGeneration = generation
    lifecycleState = .starting(generation: operationGeneration)

    let identity = nextIdentity(kind: kind)
    let factory = factory
    let cleanupDeadline = deadlines.shutdownNanoseconds
    let task = TeraRuntimeBoundedTask<TeraRuntimeBackendStart>(
      deadlineNanoseconds: deadlines.startupNanoseconds,
      operation: {
        do {
          return try await .success(factory(requestedConfiguration))
        } catch {
          return .failure(Self.failure(from: error, operation: identity.rawValue))
        }
      },
      onAbandonedResult: { result in
        guard case let .success(started) = result else { return }
        let cleanup = TeraRuntimeBoundedTask<TeraRuntimeShutdownReceipt>(
          deadlineNanoseconds: cleanupDeadline,
          operation: {
            do {
              return try await .success(started.backend.shutdown())
            } catch {
              return .failure(
                Self.failure(from: error, operation: "runtime.abandoned_startup")
              )
            }
          }
        )
        _ = await cleanup.value()
      }
    )
    let operation = StartupOperation(
      identity: identity,
      configuration: requestedConfiguration,
      task: task
    )
    startupOperation = operation
    return try await finishStartup(operation)
  }

  func retry(
    configuration requestedConfiguration: TeraRuntimeLaunchConfiguration
  ) async throws -> TeraRuntimeSnapshot {
    _ = try await stop()
    return try await start(configuration: requestedConfiguration)
  }

  func snapshot() async throws -> TeraRuntimeSnapshot {
    do {
      return try await runtimeOperation("runtime.status") { backend in
        try await backend.snapshot()
      }
    } catch let error as TeraRuntimeClientError {
      throw error
    } catch {
      throw TeraRuntimeClientError.status(
        Self.failure(from: error, operation: "runtime.status")
      )
    }
  }

  func todayPage(request: TeraTodayPageRequest) async throws -> TeraTodayPage {
    do {
      return try await runtimeOperation("runtime.today.page") { backend in
        try await backend.todayPage(request: request)
      }
    } catch let error as TeraRuntimeClientError {
      throw error
    } catch {
      throw TeraRuntimeClientError.today(
        Self.failure(from: error, operation: "runtime.today.page")
      )
    }
  }

  func refreshToday(
    context: TeraLocalNetwork,
    nowUnixSeconds: UInt64,
    update: TeraTodayProjectionUpdate = .incremental
  ) async throws -> TeraTodayRefreshReceipt {
    do {
      return try await runtimeOperation("runtime.today.refresh") { backend in
        try await backend.refreshToday(
          context: context,
          nowUnixSeconds: nowUnixSeconds,
          update: update
        )
      }
    } catch let error as TeraRuntimeClientError {
      throw error
    } catch {
      throw TeraRuntimeClientError.today(
        Self.failure(from: error, operation: "runtime.today.refresh")
      )
    }
  }

  func search(
    context: TeraLocalNetwork,
    query: String,
    limit: UInt16 = 50,
    asOfUnixSeconds: UInt64
  ) async throws -> [TeraSearchResult] {
    try await supportOperation("runtime.support.search") { backend in
      try await backend.search(
        context: context,
        query: query,
        limit: limit,
        asOfUnixSeconds: asOfUnixSeconds
      )
    }
  }

  func me(
    context: TeraLocalNetwork,
    asOfUnixSeconds: UInt64
  ) async throws -> TeraMeSnapshot {
    try await supportOperation("runtime.support.me") { backend in
      try await backend.me(context: context, asOfUnixSeconds: asOfUnixSeconds)
    }
  }

  func retrieveMedia(
    context: TeraLocalNetwork,
    reference: TeraMediaReference
  ) async throws -> TeraVerifiedMediaArtifact {
    try await supportOperation("runtime.media.retrieve") { backend in
      try await backend.retrieveMedia(context: context, reference: reference)
    }
  }

  func verifiedMediaArtifact(
    context: TeraLocalNetwork,
    artifactID: String
  ) async throws -> TeraVerifiedMediaArtifact? {
    try await supportOperation("runtime.media.verified_artifact") { backend in
      try await backend.verifiedMediaArtifact(context: context, artifactID: artifactID)
    }
  }

  func invalidateMediaArtifact(
    context: TeraLocalNetwork,
    artifactID: String
  ) async throws -> Bool {
    try await supportOperation("runtime.media.invalidate") { backend in
      try await backend.invalidateMediaArtifact(context: context, artifactID: artifactID)
    }
  }

  func addSchemas() async throws -> [TeraAddSchema] {
    try await addOperation("runtime.add.schemas") { backend in
      try await backend.addSchemas()
    }
  }

  func saveAddIntent(
    input: TeraAddRuntimeInput,
    existingDraftID: String?,
    expectedRevision: UInt64?
  ) async throws -> TeraDraftStatus {
    try await addOperation("runtime.add.save") { backend in
      try await backend.saveAddIntent(
        input: input,
        existingDraftID: existingDraftID,
        expectedRevision: expectedRevision
      )
    }
  }

  func saveRetractionDraft(
    id: String,
    input: TeraRetractionDraftInput,
    authoredAtUnixSeconds: UInt64,
    persistedAtUnixMilliseconds: UInt64
  ) async throws -> TeraDraftStatus {
    try await addOperation("runtime.add.retract") { backend in
      try await backend.saveRetractionDraft(
        id: id,
        input: input,
        authoredAtUnixSeconds: authoredAtUnixSeconds,
        persistedAtUnixMilliseconds: persistedAtUnixMilliseconds
      )
    }
  }

  func saveRevisionIntent(
    target: TeraRevisionTarget,
    replacement: TeraAddRuntimeInput
  ) async throws -> TeraRevisionStatus {
    try await addOperation("runtime.add.revision.save") { backend in
      try await backend.saveRevisionIntent(target: target, replacement: replacement)
    }
  }

  func revisionStatus(operationID: String) async throws -> TeraRevisionStatus {
    try await addOperation("runtime.add.revision.status") { backend in
      try await backend.revisionStatus(operationID: operationID)
    }
  }

  func advanceRevision(operationID: String) async throws -> TeraRevisionStatus {
    try await addOperation("runtime.add.revision.advance") { backend in
      try await backend.advanceRevision(operationID: operationID)
    }
  }

  func cancelRevision(operationID: String) async throws -> TeraRevisionStatus {
    try await addOperation("runtime.add.revision.cancel") { backend in
      try await backend.cancelRevision(operationID: operationID)
    }
  }

  func draftStatus(id: String) async throws -> TeraDraftStatus {
    try await addOperation("runtime.add.status") { backend in
      try await backend.draftStatus(id: id)
    }
  }

  func draftHeads(limit: UInt16 = 100) async throws -> [TeraDraftStatus] {
    try await addOperation("runtime.add.heads") { backend in
      try await backend.draftHeads(limit: limit)
    }
  }

  func queueAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> TeraDraftStatus {
    try await addOperation("runtime.add.queue") { backend in
      try await backend.queueAddIntent(
        id: id,
        expectedRevision: expectedRevision
      )
    }
  }

  func recoverAddIntent(id: String) async throws -> TeraDraftStatus {
    try await addOperation("runtime.add.recover") { backend in
      try await backend.recoverAddIntent(id: id)
    }
  }

  func uploadAddMediaIntent(input: TeraBlossomUploadIntent) async throws -> TeraDraftStatus {
    try await addOperation("runtime.add.media") { backend in
      try await backend.uploadAddMediaIntent(input: input)
    }
  }

  func prepareAddMediaBackground(
    input: TeraBlossomUploadIntent
  ) async throws -> TeraNativeUploadJob {
    try await addOperation("runtime.add.media.background.prepare") { backend in
      try await backend.prepareAddMediaBackground(input: input)
    }
  }

  func completeAddMediaBackground(
    input: TeraNativeUploadCompletion
  ) async throws -> TeraDraftStatus {
    try await addOperation("runtime.add.media.background.complete") { backend in
      try await backend.completeAddMediaBackground(input: input)
    }
  }

  func probeBlossom() async throws -> TeraBlossomEvidence {
    try await supportOperation("runtime.blossom.probe") { backend in
      try await backend.probeBlossom()
    }
  }

  func mobileSettings() async throws -> TeraMobileSettings {
    try await supportOperation("runtime.settings.read") { backend in
      try await backend.mobileSettings()
    }
  }

  func replaceMobileSettings(
    input: TeraReplaceSettings
  ) async throws -> TeraSettingsTransition {
    try await supportOperation("runtime.settings.replace") { backend in
      try await backend.replaceMobileSettings(input: input)
    }
  }

  func applyIdentityCommand(
    expectedRevision: UInt64,
    command: TeraIdentityCommand
  ) async throws -> TeraSettingsTransition {
    try await supportOperation("runtime.settings.identity") { backend in
      try await backend.applyIdentityCommand(
        expectedRevision: expectedRevision,
        command: command
      )
    }
  }

  func saveProfileMetadata(input: TeraProfileMetadataInput) async throws
    -> TeraProfileStatus
  {
    try await supportOperation("runtime.profile.save") { backend in
      try await backend.saveProfileMetadata(input: input)
    }
  }

  func profileStatus(operationID: String) async throws -> TeraProfileStatus {
    try await supportOperation("runtime.profile.status") { backend in
      try await backend.profileStatus(operationID: operationID)
    }
  }

  func advanceProfile(operationID: String) async throws -> TeraProfileStatus {
    try await supportOperation("runtime.profile.advance") { backend in
      try await backend.advanceProfile(operationID: operationID)
    }
  }

  func cancelProfile(
    operationID: String,
    expectedRevision: UInt64
  ) async throws -> TeraProfileStatus {
    try await supportOperation("runtime.profile.cancel") { backend in
      try await backend.cancelProfile(
        operationID: operationID,
        expectedRevision: expectedRevision
      )
    }
  }

  func advanceDraft(id: String, expectedRevision: UInt64) async throws -> TeraDraftStatus {
    try await addOperation("runtime.add.advance") { backend in
      try await backend.advanceDraft(id: id, expectedRevision: expectedRevision)
    }
  }

  func cancelAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> TeraDraftStatus {
    try await addOperation("runtime.add.cancel") { backend in
      try await backend.cancelAddIntent(
        id: id,
        expectedRevision: expectedRevision
      )
    }
  }

  func changes(bufferCapacity: Int = 16) async throws -> AsyncStream<TeraRuntimeChange> {
    guard (1 ... 64).contains(bufferCapacity) else {
      throw TeraRuntimeClientError.invalidBufferCapacity
    }
    guard let backend, case .running = lifecycleState else {
      throw TeraRuntimeClientError.notRunning
    }

    let id = UUID()
    let subscriptionGeneration = generation
    let identity = nextIdentity(kind: .subscription)
    let pair = AsyncStream.makeStream(
      of: TeraRuntimeChange.self,
      bufferingPolicy: .bufferingNewest(bufferCapacity)
    )
    pair.continuation.onTermination = { [weak self] _ in
      Task {
        await self?.cancelSubscription(id: id, generation: subscriptionGeneration)
      }
    }
    subscriptions[id] = Subscription(
      generation: subscriptionGeneration,
      continuation: pair.continuation,
      token: nil
    )

    let subscriptionDeadline = deadlines.subscriptionNanoseconds
    let task = TeraRuntimeBoundedTask<any TeraRuntimeSubscriptionToken>(
      deadlineNanoseconds: subscriptionDeadline,
      operation: {
        do {
          return try await .success(
            backend.subscribe(bufferCapacity: bufferCapacity) { [weak self] change in
              await self?.receive(
                change,
                subscriptionID: id,
                generation: subscriptionGeneration
              )
            }
          )
        } catch {
          return .failure(Self.failure(from: error, operation: identity.rawValue))
        }
      },
      onAbandonedResult: { result in
        guard case let .success(token) = result else { return }
        let cancellation = TeraRuntimeBoundedTask<Void>(
          deadlineNanoseconds: subscriptionDeadline,
          operation: {
            await token.cancel()
            return .success(())
          }
        )
        _ = await cancellation.value()
      }
    )
    activeOperations[identity.sequence] = ActiveOperation(
      identity: identity,
      cancel: { task.cancel() }
    )
    let outcome = await task.value()
    removeActiveOperation(identity)

    guard generation == subscriptionGeneration,
          case .running = lifecycleState,
          var subscription = subscriptions[id]
    else {
      subscriptions.removeValue(forKey: id)?.continuation.finish()
      if case let .completed(.success(token)) = outcome {
        cancelTokenDetached(token)
      }
      throw TeraRuntimeClientError.superseded
    }

    switch outcome {
    case let .completed(.success(token)):
      subscription.token = token
      subscriptions[id] = subscription
      return pair.stream
    case let .completed(.failure(failure)):
      subscriptions.removeValue(forKey: id)?.continuation.finish()
      throw TeraRuntimeClientError.subscription(failure)
    case .timedOut:
      subscriptions.removeValue(forKey: id)?.continuation.finish()
      throw TeraRuntimeClientError.subscription(
        Self.deadlineFailure(identity: identity)
      )
    case .cancelled:
      subscriptions.removeValue(forKey: id)?.continuation.finish()
      throw TeraRuntimeClientError.subscription(
        Self.cancellationFailure(identity: identity)
      )
    }
  }

  func suspend() {
    for operation in activeOperations.values {
      operation.cancel()
    }
    activeOperations.removeAll()

    let activeSubscriptions = Array(subscriptions.values)
    subscriptions.removeAll()
    for subscription in activeSubscriptions {
      subscription.continuation.finish()
      if let token = subscription.token {
        cancelTokenDetached(token)
      }
    }

    if let startupOperation {
      generation = generation.invalidated()
      startupOperation.task.cancel()
      self.startupOperation = nil
      configuration = nil
      lifecycleState = .stopped
    }
  }

  func stop() async throws -> TeraRuntimeShutdownReceipt {
    if let shutdownOperation {
      return try await finishShutdown(shutdownOperation)
    }
    guard
      startupOperation != nil || backend != nil || quarantinedBackend != nil
      || !subscriptions.isEmpty || !activeOperations.isEmpty
    else {
      lifecycleState = .stopped
      return .alreadyStopped
    }
    return try await finishShutdown(beginShutdown())
  }

  private func finishStartup(
    _ operation: StartupOperation
  ) async throws -> TeraRuntimeSnapshot {
    let outcome = await operation.task.value(cancelsOperationWhenWaiterCancelled: false)
    guard generation == operation.identity.generation else {
      throw TeraRuntimeClientError.superseded
    }

    if case .cancelled = outcome,
       startupOperation?.identity == operation.identity
    {
      throw TeraRuntimeClientError.startup(
        Self.cancellationFailure(identity: operation.identity)
      )
    }

    if startupOperation?.identity == operation.identity {
      startupOperation = nil
    } else if case let .completed(.success(started)) = outcome,
              configuration == operation.configuration,
              case .running = lifecycleState
    {
      return started.snapshot
    } else {
      throw TeraRuntimeClientError.superseded
    }

    switch outcome {
    case let .completed(.success(started)):
      backend = started.backend
      configuration = operation.configuration
      lifecycleState = .running(generation: operation.identity.generation)
      return started.snapshot
    case let .completed(.failure(failure)):
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw TeraRuntimeClientError.startup(failure)
    case .timedOut:
      let failure = Self.deadlineFailure(identity: operation.identity)
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw TeraRuntimeClientError.startup(failure)
    case .cancelled:
      let failure = Self.cancellationFailure(identity: operation.identity)
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw TeraRuntimeClientError.startup(failure)
    }
  }

  private func beginShutdown() -> ShutdownOperation {
    generation = generation.invalidated()
    let operationGeneration = generation
    let pendingStartup = startupOperation
    let activeBackend = backend ?? quarantinedBackend
    let activeSubscriptions = Array(subscriptions.values)
    let activeRuntimeOperations = Array(activeOperations.values)

    startupOperation = nil
    backend = nil
    quarantinedBackend = nil
    configuration = nil
    subscriptions.removeAll()
    activeOperations.removeAll()
    lifecycleState = .stopping(generation: operationGeneration)

    pendingStartup?.task.cancel()
    for operation in activeRuntimeOperations {
      operation.cancel()
    }
    for subscription in activeSubscriptions {
      subscription.continuation.finish()
    }

    let identity = nextIdentity(kind: .shutdown)
    let cancellationDeadline = deadlines.subscriptionNanoseconds
    let task = TeraRuntimeBoundedTask<TeraRuntimeShutdownReceipt>(
      deadlineNanoseconds: deadlines.shutdownNanoseconds,
      operation: {
        let cancellations = activeSubscriptions.compactMap(\.token).map { token in
          TeraRuntimeBoundedTask<Void>(
            deadlineNanoseconds: cancellationDeadline,
            operation: {
              await token.cancel()
              return .success(())
            }
          )
        }
        for cancellation in cancellations {
          _ = await cancellation.value()
        }

        guard let activeBackend else {
          return .success(.alreadyStopped)
        }
        do {
          return try await .success(activeBackend.shutdown())
        } catch {
          return .failure(Self.failure(from: error, operation: identity.rawValue))
        }
      },
      onAbandonedResult: { [weak self] result in
        await self?.settleLateShutdown(identity: identity, result: result)
      }
    )
    let operation = ShutdownOperation(identity: identity, backend: activeBackend, task: task)
    shutdownOperation = operation
    return operation
  }

  private func finishShutdown(
    _ operation: ShutdownOperation
  ) async throws -> TeraRuntimeShutdownReceipt {
    let outcome = await operation.task.value(cancelsOperationWhenWaiterCancelled: false)
    if case .cancelled = outcome,
       shutdownOperation?.identity == operation.identity,
       generation == operation.identity.generation
    {
      throw TeraRuntimeClientError.shutdown(
        Self.cancellationFailure(identity: operation.identity)
      )
    }
    if shutdownOperation?.identity == operation.identity {
      shutdownOperation = nil
    }

    let lateSucceeded = lateShutdownSuccesses.remove(operation.identity) != nil
    guard generation == operation.identity.generation else {
      throw TeraRuntimeClientError.superseded
    }

    switch outcome {
    case let .completed(.success(receipt)):
      quarantinedBackend = nil
      lifecycleState = .stopped
      return receipt
    case let .completed(.failure(failure)):
      quarantinedBackend = operation.backend
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw TeraRuntimeClientError.shutdown(failure)
    case .timedOut:
      let failure = Self.deadlineFailure(identity: operation.identity)
      quarantinedBackend = lateSucceeded ? nil : operation.backend
      lifecycleState =
        lateSucceeded
          ? .stopped
          : .failed(generation: operation.identity.generation, failure: failure)
      throw TeraRuntimeClientError.shutdown(failure)
    case .cancelled:
      let failure = Self.cancellationFailure(identity: operation.identity)
      quarantinedBackend = lateSucceeded ? nil : operation.backend
      lifecycleState =
        lateSucceeded
          ? .stopped
          : .failed(generation: operation.identity.generation, failure: failure)
      throw TeraRuntimeClientError.shutdown(failure)
    }
  }

  private func runtimeOperation<T: Sendable>(
    _ operation: String,
    _ body: @escaping @Sendable (any TeraRuntimeBackend) async throws -> T
  ) async throws -> T {
    guard let backend, case .running = lifecycleState else {
      throw TeraRuntimeClientError.notRunning
    }
    let operationGeneration = generation
    let identity = nextIdentity(kind: .operation)
    let task = TeraRuntimeBoundedTask<T>(
      deadlineNanoseconds: deadlines.operationNanoseconds,
      operation: {
        do {
          return try await .success(body(backend))
        } catch {
          return .failure(Self.failure(from: error, operation: operation))
        }
      }
    )
    activeOperations[identity.sequence] = ActiveOperation(
      identity: identity,
      cancel: { task.cancel() }
    )
    let outcome = await task.value()
    removeActiveOperation(identity)

    guard generation == operationGeneration, case .running = lifecycleState else {
      throw TeraRuntimeClientError.superseded
    }
    switch outcome {
    case let .completed(.success(value)):
      return value
    case let .completed(.failure(failure)):
      throw failure
    case .timedOut:
      throw Self.deadlineFailure(identity: identity)
    case .cancelled:
      throw Self.cancellationFailure(identity: identity)
    }
  }

  private func addOperation<T: Sendable>(
    _ operation: String,
    _ body: @escaping @Sendable (any TeraRuntimeBackend) async throws -> T
  ) async throws -> T {
    do {
      return try await runtimeOperation(operation, body)
    } catch let error as TeraRuntimeClientError {
      throw error
    } catch {
      throw TeraRuntimeClientError.add(
        Self.failure(from: error, operation: operation)
      )
    }
  }

  private func supportOperation<T: Sendable>(
    _ operation: String,
    _ body: @escaping @Sendable (any TeraRuntimeBackend) async throws -> T
  ) async throws -> T {
    do {
      return try await runtimeOperation(operation, body)
    } catch let error as TeraRuntimeClientError {
      throw error
    } catch {
      throw TeraRuntimeClientError.support(
        Self.failure(from: error, operation: operation)
      )
    }
  }

  private func receive(
    _ change: TeraRuntimeChange,
    subscriptionID: UUID,
    generation subscriptionGeneration: TeraSessionGeneration
  ) {
    guard generation == subscriptionGeneration,
          case .running = lifecycleState,
          let subscription = subscriptions[subscriptionID],
          subscription.generation == subscriptionGeneration
    else {
      return
    }
    subscription.continuation.yield(change)
  }

  private func cancelSubscription(id: UUID, generation subscriptionGeneration: TeraSessionGeneration) {
    guard let subscription = subscriptions[id],
          subscription.generation == subscriptionGeneration
    else {
      return
    }
    subscriptions.removeValue(forKey: id)
    subscription.continuation.finish()
    if let token = subscription.token {
      cancelTokenDetached(token)
    }
  }

  private func cancelTokenDetached(_ token: any TeraRuntimeSubscriptionToken) {
    let deadline = deadlines.subscriptionNanoseconds
    Task {
      let cancellation = TeraRuntimeBoundedTask<Void>(
        deadlineNanoseconds: deadline,
        operation: {
          await token.cancel()
          return .success(())
        }
      )
      _ = await cancellation.value()
    }
  }

  private func settleLateShutdown(
    identity: TeraRuntimeOperationIdentity,
    result: Result<TeraRuntimeShutdownReceipt, TeraRuntimeFailure>
  ) {
    guard generation == identity.generation else { return }
    guard case .success = result else { return }
    if shutdownOperation?.identity == identity {
      lateShutdownSuccesses.insert(identity)
    } else {
      quarantinedBackend = nil
      lifecycleState = .stopped
    }
  }

  private func nextIdentity(
    kind: TeraRuntimeOperationKind
  ) -> TeraRuntimeOperationIdentity {
    operationSequence &+= 1
    return TeraRuntimeOperationIdentity(
      generation: generation,
      sequence: operationSequence,
      kind: kind
    )
  }

  private func removeActiveOperation(_ identity: TeraRuntimeOperationIdentity) {
    guard activeOperations[identity.sequence]?.identity == identity else { return }
    activeOperations.removeValue(forKey: identity.sequence)
  }

  private static func deadlineFailure(
    identity: TeraRuntimeOperationIdentity
  ) -> TeraRuntimeFailure {
    .local(
      operation: identity.rawValue,
      code: "ios.runtime.deadline_exceeded",
      safeMessage: "The Tera runtime operation did not finish in time."
    )
  }

  private static func cancellationFailure(
    identity: TeraRuntimeOperationIdentity
  ) -> TeraRuntimeFailure {
    .local(
      operation: identity.rawValue,
      code: "ios.runtime.cancelled",
      safeMessage: "The Tera runtime operation was cancelled."
    )
  }

  private static func failure(from error: Error, operation: String) -> TeraRuntimeFailure {
    if let failure = error as? TeraRuntimeFailure {
      return failure
    }
    if error is CancellationError {
      return .local(
        operation: operation,
        code: "ios.runtime.cancelled",
        safeMessage: "The Tera runtime operation was cancelled."
      )
    }
    if case let TeraRuntimeClientError.startup(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.subscription(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.status(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.today(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.add(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.support(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.shutdown(failure) = error {
      return failure
    }
    return .local(
      operation: operation,
      code: "ios.runtime.unexpected",
      safeMessage: "The Tera runtime could not complete the operation."
    )
  }
}
