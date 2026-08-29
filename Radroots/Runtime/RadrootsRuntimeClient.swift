import Foundation

protocol RadrootsRuntimeSubscriptionToken: Sendable {
  func cancel() async
}

protocol RadrootsRuntimeBackend: Sendable {
  func snapshot() async throws -> RadrootsRuntimeSnapshot
  func todayPage(request: RadrootsTodayPageRequest) async throws -> RadrootsTodayPage
  func refreshToday(
    context: RadrootsLocalNetwork,
    nowUnixSeconds: UInt64,
    update: RadrootsTodayProjectionUpdate
  ) async throws -> RadrootsTodayRefreshReceipt
  func search(
    context: RadrootsLocalNetwork,
    query: String,
    limit: UInt16,
    asOfUnixSeconds: UInt64
  ) async throws -> [RadrootsSearchResult]
  func me(
    context: RadrootsLocalNetwork,
    asOfUnixSeconds: UInt64
  ) async throws -> RadrootsMeSnapshot
  func retrieveMedia(
    context: RadrootsLocalNetwork,
    reference: RadrootsMediaReference
  ) async throws -> RadrootsVerifiedMediaArtifact
  func verifiedMediaArtifact(
    context: RadrootsLocalNetwork,
    artifactID: String
  ) async throws -> RadrootsVerifiedMediaArtifact?
  func invalidateMediaArtifact(
    context: RadrootsLocalNetwork,
    artifactID: String
  ) async throws -> Bool
  func addSchemas() async throws -> [RadrootsAddSchema]
  func saveAddIntent(
    input: RadrootsAddRuntimeInput,
    existingDraftID: String?,
    expectedRevision: UInt64?
  ) async throws -> RadrootsDraftStatus
  func saveRetractionDraft(
    id: String,
    input: RadrootsRetractionDraftInput,
    authoredAtUnixSeconds: UInt64,
    persistedAtUnixMilliseconds: UInt64
  ) async throws -> RadrootsDraftStatus
  func saveRevisionIntent(
    target: RadrootsRevisionTarget,
    replacement: RadrootsAddRuntimeInput
  ) async throws -> RadrootsRevisionStatus
  func revisionStatus(operationID: String) async throws -> RadrootsRevisionStatus
  func advanceRevision(operationID: String) async throws -> RadrootsRevisionStatus
  func cancelRevision(operationID: String) async throws -> RadrootsRevisionStatus
  func draftStatus(id: String) async throws -> RadrootsDraftStatus
  func draftHeads(limit: UInt16) async throws -> [RadrootsDraftStatus]
  func queueAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> RadrootsDraftStatus
  func recoverAddIntent(id: String) async throws -> RadrootsDraftStatus
  func uploadAddMediaIntent(input: RadrootsBlossomUploadIntent) async throws -> RadrootsDraftStatus
  func prepareAddMediaBackground(
    input: RadrootsBlossomUploadIntent
  ) async throws -> RadrootsNativeUploadJob
  func completeAddMediaBackground(
    input: RadrootsNativeUploadCompletion
  ) async throws -> RadrootsDraftStatus
  func probeBlossom() async throws -> RadrootsBlossomEvidence
  func mobileSettings() async throws -> RadrootsMobileSettings
  func replaceMobileSettings(
    input: RadrootsReplaceSettings
  ) async throws -> RadrootsSettingsTransition
  func applyIdentityCommand(
    expectedRevision: UInt64,
    command: RadrootsIdentityCommand
  ) async throws -> RadrootsSettingsTransition
  func saveProfileMetadata(input: RadrootsProfileMetadataInput) async throws
    -> RadrootsProfileStatus
  func profileStatus(operationID: String) async throws -> RadrootsProfileStatus
  func advanceProfile(operationID: String) async throws -> RadrootsProfileStatus
  func cancelProfile(operationID: String, expectedRevision: UInt64) async throws
    -> RadrootsProfileStatus
  func advanceDraft(id: String, expectedRevision: UInt64) async throws -> RadrootsDraftStatus
  func cancelAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> RadrootsDraftStatus
  func subscribe(
    bufferCapacity: Int,
    receive: @escaping @Sendable (RadrootsRuntimeChange) async -> Void
  ) async throws -> any RadrootsRuntimeSubscriptionToken
  func shutdown() async throws -> RadrootsRuntimeShutdownReceipt
}

extension RadrootsRuntimeBackend {
  private func supportUnsupported() -> RadrootsRuntimeFailure {
    .local(
      operation: "runtime.support",
      code: "ios.support.unsupported",
      safeMessage: "This supporting surface is unavailable in the current runtime."
    )
  }

  private func addUnsupported() -> RadrootsRuntimeFailure {
    .local(
      operation: "runtime.add",
      code: "ios.add.unsupported",
      safeMessage: "Add is unavailable in this runtime."
    )
  }

  func addSchemas() async throws -> [RadrootsAddSchema] {
    throw addUnsupported()
  }

  func saveAddIntent(
    input _: RadrootsAddRuntimeInput,
    existingDraftID _: String?,
    expectedRevision _: UInt64?
  ) async throws -> RadrootsDraftStatus {
    throw addUnsupported()
  }

  func saveRetractionDraft(
    id _: String,
    input _: RadrootsRetractionDraftInput,
    authoredAtUnixSeconds _: UInt64,
    persistedAtUnixMilliseconds _: UInt64
  ) async throws -> RadrootsDraftStatus {
    throw addUnsupported()
  }

  func saveRevisionIntent(
    target _: RadrootsRevisionTarget,
    replacement _: RadrootsAddRuntimeInput
  ) async throws -> RadrootsRevisionStatus {
    throw addUnsupported()
  }

  func revisionStatus(operationID _: String) async throws -> RadrootsRevisionStatus {
    throw addUnsupported()
  }

  func advanceRevision(operationID _: String) async throws -> RadrootsRevisionStatus {
    throw addUnsupported()
  }

  func cancelRevision(operationID _: String) async throws -> RadrootsRevisionStatus {
    throw addUnsupported()
  }

  func draftStatus(id _: String) async throws -> RadrootsDraftStatus {
    throw addUnsupported()
  }

  func draftHeads(limit _: UInt16) async throws -> [RadrootsDraftStatus] {
    throw addUnsupported()
  }

  func queueAddIntent(
    id _: String,
    expectedRevision _: UInt64
  ) async throws -> RadrootsDraftStatus {
    throw addUnsupported()
  }

  func recoverAddIntent(id _: String) async throws -> RadrootsDraftStatus {
    throw addUnsupported()
  }

  func uploadAddMediaIntent(input _: RadrootsBlossomUploadIntent) async throws
    -> RadrootsDraftStatus
  {
    throw addUnsupported()
  }

  func prepareAddMediaBackground(
    input _: RadrootsBlossomUploadIntent
  ) async throws -> RadrootsNativeUploadJob {
    throw addUnsupported()
  }

  func completeAddMediaBackground(
    input _: RadrootsNativeUploadCompletion
  ) async throws -> RadrootsDraftStatus {
    throw addUnsupported()
  }

  func probeBlossom() async throws -> RadrootsBlossomEvidence {
    throw supportUnsupported()
  }

  func mobileSettings() async throws -> RadrootsMobileSettings {
    throw supportUnsupported()
  }

  func replaceMobileSettings(
    input _: RadrootsReplaceSettings
  ) async throws -> RadrootsSettingsTransition {
    throw supportUnsupported()
  }

  func applyIdentityCommand(
    expectedRevision _: UInt64,
    command _: RadrootsIdentityCommand
  ) async throws -> RadrootsSettingsTransition {
    throw supportUnsupported()
  }

  func saveProfileMetadata(input _: RadrootsProfileMetadataInput) async throws
    -> RadrootsProfileStatus
  {
    throw supportUnsupported()
  }

  func profileStatus(operationID _: String) async throws -> RadrootsProfileStatus {
    throw supportUnsupported()
  }

  func advanceProfile(operationID _: String) async throws -> RadrootsProfileStatus {
    throw supportUnsupported()
  }

  func cancelProfile(operationID _: String, expectedRevision _: UInt64) async throws
    -> RadrootsProfileStatus
  {
    throw supportUnsupported()
  }

  func advanceDraft(id _: String, expectedRevision _: UInt64) async throws -> RadrootsDraftStatus {
    throw addUnsupported()
  }

  func cancelAddIntent(
    id _: String,
    expectedRevision _: UInt64
  ) async throws -> RadrootsDraftStatus {
    throw addUnsupported()
  }

  func search(
    context _: RadrootsLocalNetwork,
    query _: String,
    limit _: UInt16,
    asOfUnixSeconds _: UInt64
  ) async throws -> [RadrootsSearchResult] {
    throw supportUnsupported()
  }

  func me(
    context _: RadrootsLocalNetwork,
    asOfUnixSeconds _: UInt64
  ) async throws -> RadrootsMeSnapshot {
    throw supportUnsupported()
  }

  func retrieveMedia(
    context _: RadrootsLocalNetwork,
    reference _: RadrootsMediaReference
  ) async throws -> RadrootsVerifiedMediaArtifact {
    throw supportUnsupported()
  }

  func verifiedMediaArtifact(
    context _: RadrootsLocalNetwork,
    artifactID _: String
  ) async throws -> RadrootsVerifiedMediaArtifact? {
    throw supportUnsupported()
  }

  func invalidateMediaArtifact(
    context _: RadrootsLocalNetwork,
    artifactID _: String
  ) async throws -> Bool {
    throw supportUnsupported()
  }
}

struct RadrootsRuntimeBackendStart: Sendable {
  let backend: any RadrootsRuntimeBackend
  let snapshot: RadrootsRuntimeSnapshot
}

typealias RadrootsRuntimeBackendFactory =
  @Sendable (
    RadrootsRuntimeLaunchConfiguration
  ) async throws -> RadrootsRuntimeBackendStart

private enum RadrootsRuntimeBoundedOutcome<Value: Sendable>: Sendable {
  case completed(Result<Value, RadrootsRuntimeFailure>)
  case timedOut
  case cancelled
}

private final class RadrootsRuntimeBoundedTask<Value: Sendable>: @unchecked Sendable {
  typealias Outcome = RadrootsRuntimeBoundedOutcome<Value>

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
    operation: @escaping @Sendable () async -> Result<Value, RadrootsRuntimeFailure>,
    onAbandonedResult:
    @escaping @Sendable (
      Result<Value, RadrootsRuntimeFailure>
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

actor RadrootsRuntimeClient {
  private struct StartupOperation: Sendable {
    let identity: RadrootsRuntimeOperationIdentity
    let configuration: RadrootsRuntimeLaunchConfiguration
    let task: RadrootsRuntimeBoundedTask<RadrootsRuntimeBackendStart>
  }

  private struct ShutdownOperation: Sendable {
    let identity: RadrootsRuntimeOperationIdentity
    let backend: (any RadrootsRuntimeBackend)?
    let task: RadrootsRuntimeBoundedTask<RadrootsRuntimeShutdownReceipt>
  }

  private struct ActiveOperation: Sendable {
    let identity: RadrootsRuntimeOperationIdentity
    let cancel: @Sendable () -> Void
  }

  private struct Subscription {
    let generation: UInt64
    let continuation: AsyncStream<RadrootsRuntimeChange>.Continuation
    var token: (any RadrootsRuntimeSubscriptionToken)?
  }

  private let factory: RadrootsRuntimeBackendFactory
  private let deadlines: RadrootsRuntimeDeadlinePolicy
  private var generation: UInt64 = 0
  private var operationSequence: UInt64 = 0
  private var lifecycleState: RadrootsRuntimeLifecycle = .stopped
  private var configuration: RadrootsRuntimeLaunchConfiguration?
  private var backend: (any RadrootsRuntimeBackend)?
  private var quarantinedBackend: (any RadrootsRuntimeBackend)?
  private var startupOperation: StartupOperation?
  private var shutdownOperation: ShutdownOperation?
  private var activeOperations: [UInt64: ActiveOperation] = [:]
  private var subscriptions: [UUID: Subscription] = [:]
  private var lateShutdownSuccesses: Set<RadrootsRuntimeOperationIdentity> = []

  init(
    factory: @escaping RadrootsRuntimeBackendFactory,
    deadlines: RadrootsRuntimeDeadlinePolicy = .production
  ) {
    self.factory = factory
    self.deadlines = deadlines
  }

  func lifecycle() -> RadrootsRuntimeLifecycle {
    lifecycleState
  }

  func start(
    configuration requestedConfiguration: RadrootsRuntimeLaunchConfiguration
  ) async throws -> RadrootsRuntimeSnapshot {
    try await start(configuration: requestedConfiguration, kind: .startup)
  }

  func reconfigure(
    configuration requestedConfiguration: RadrootsRuntimeLaunchConfiguration
  ) async throws -> RadrootsRuntimeSnapshot {
    try await start(configuration: requestedConfiguration, kind: .reconfiguration)
  }

  private func start(
    configuration requestedConfiguration: RadrootsRuntimeLaunchConfiguration,
    kind: RadrootsRuntimeOperationKind
  ) async throws -> RadrootsRuntimeSnapshot {
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

    generation &+= 1
    let operationGeneration = generation
    lifecycleState = .starting(generation: operationGeneration)

    let identity = nextIdentity(kind: kind)
    let factory = factory
    let cleanupDeadline = deadlines.shutdownNanoseconds
    let task = RadrootsRuntimeBoundedTask<RadrootsRuntimeBackendStart>(
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
        let cleanup = RadrootsRuntimeBoundedTask<RadrootsRuntimeShutdownReceipt>(
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
    configuration requestedConfiguration: RadrootsRuntimeLaunchConfiguration
  ) async throws -> RadrootsRuntimeSnapshot {
    _ = try await stop()
    return try await start(configuration: requestedConfiguration)
  }

  func snapshot() async throws -> RadrootsRuntimeSnapshot {
    do {
      return try await runtimeOperation("runtime.status") { backend in
        try await backend.snapshot()
      }
    } catch let error as RadrootsRuntimeClientError {
      throw error
    } catch {
      throw RadrootsRuntimeClientError.status(
        Self.failure(from: error, operation: "runtime.status")
      )
    }
  }

  func todayPage(request: RadrootsTodayPageRequest) async throws -> RadrootsTodayPage {
    do {
      return try await runtimeOperation("runtime.today.page") { backend in
        try await backend.todayPage(request: request)
      }
    } catch let error as RadrootsRuntimeClientError {
      throw error
    } catch {
      throw RadrootsRuntimeClientError.today(
        Self.failure(from: error, operation: "runtime.today.page")
      )
    }
  }

  func refreshToday(
    context: RadrootsLocalNetwork,
    nowUnixSeconds: UInt64,
    update: RadrootsTodayProjectionUpdate = .incremental
  ) async throws -> RadrootsTodayRefreshReceipt {
    do {
      return try await runtimeOperation("runtime.today.refresh") { backend in
        try await backend.refreshToday(
          context: context,
          nowUnixSeconds: nowUnixSeconds,
          update: update
        )
      }
    } catch let error as RadrootsRuntimeClientError {
      throw error
    } catch {
      throw RadrootsRuntimeClientError.today(
        Self.failure(from: error, operation: "runtime.today.refresh")
      )
    }
  }

  func search(
    context: RadrootsLocalNetwork,
    query: String,
    limit: UInt16 = 50,
    asOfUnixSeconds: UInt64
  ) async throws -> [RadrootsSearchResult] {
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
    context: RadrootsLocalNetwork,
    asOfUnixSeconds: UInt64
  ) async throws -> RadrootsMeSnapshot {
    try await supportOperation("runtime.support.me") { backend in
      try await backend.me(context: context, asOfUnixSeconds: asOfUnixSeconds)
    }
  }

  func retrieveMedia(
    context: RadrootsLocalNetwork,
    reference: RadrootsMediaReference
  ) async throws -> RadrootsVerifiedMediaArtifact {
    try await supportOperation("runtime.media.retrieve") { backend in
      try await backend.retrieveMedia(context: context, reference: reference)
    }
  }

  func verifiedMediaArtifact(
    context: RadrootsLocalNetwork,
    artifactID: String
  ) async throws -> RadrootsVerifiedMediaArtifact? {
    try await supportOperation("runtime.media.verified_artifact") { backend in
      try await backend.verifiedMediaArtifact(context: context, artifactID: artifactID)
    }
  }

  func invalidateMediaArtifact(
    context: RadrootsLocalNetwork,
    artifactID: String
  ) async throws -> Bool {
    try await supportOperation("runtime.media.invalidate") { backend in
      try await backend.invalidateMediaArtifact(context: context, artifactID: artifactID)
    }
  }

  func addSchemas() async throws -> [RadrootsAddSchema] {
    try await addOperation("runtime.add.schemas") { backend in
      try await backend.addSchemas()
    }
  }

  func saveAddIntent(
    input: RadrootsAddRuntimeInput,
    existingDraftID: String?,
    expectedRevision: UInt64?
  ) async throws -> RadrootsDraftStatus {
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
    input: RadrootsRetractionDraftInput,
    authoredAtUnixSeconds: UInt64,
    persistedAtUnixMilliseconds: UInt64
  ) async throws -> RadrootsDraftStatus {
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
    target: RadrootsRevisionTarget,
    replacement: RadrootsAddRuntimeInput
  ) async throws -> RadrootsRevisionStatus {
    try await addOperation("runtime.add.revision.save") { backend in
      try await backend.saveRevisionIntent(target: target, replacement: replacement)
    }
  }

  func revisionStatus(operationID: String) async throws -> RadrootsRevisionStatus {
    try await addOperation("runtime.add.revision.status") { backend in
      try await backend.revisionStatus(operationID: operationID)
    }
  }

  func advanceRevision(operationID: String) async throws -> RadrootsRevisionStatus {
    try await addOperation("runtime.add.revision.advance") { backend in
      try await backend.advanceRevision(operationID: operationID)
    }
  }

  func cancelRevision(operationID: String) async throws -> RadrootsRevisionStatus {
    try await addOperation("runtime.add.revision.cancel") { backend in
      try await backend.cancelRevision(operationID: operationID)
    }
  }

  func draftStatus(id: String) async throws -> RadrootsDraftStatus {
    try await addOperation("runtime.add.status") { backend in
      try await backend.draftStatus(id: id)
    }
  }

  func draftHeads(limit: UInt16 = 100) async throws -> [RadrootsDraftStatus] {
    try await addOperation("runtime.add.heads") { backend in
      try await backend.draftHeads(limit: limit)
    }
  }

  func queueAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> RadrootsDraftStatus {
    try await addOperation("runtime.add.queue") { backend in
      try await backend.queueAddIntent(
        id: id,
        expectedRevision: expectedRevision
      )
    }
  }

  func recoverAddIntent(id: String) async throws -> RadrootsDraftStatus {
    try await addOperation("runtime.add.recover") { backend in
      try await backend.recoverAddIntent(id: id)
    }
  }

  func uploadAddMediaIntent(input: RadrootsBlossomUploadIntent) async throws -> RadrootsDraftStatus {
    try await addOperation("runtime.add.media") { backend in
      try await backend.uploadAddMediaIntent(input: input)
    }
  }

  func prepareAddMediaBackground(
    input: RadrootsBlossomUploadIntent
  ) async throws -> RadrootsNativeUploadJob {
    try await addOperation("runtime.add.media.background.prepare") { backend in
      try await backend.prepareAddMediaBackground(input: input)
    }
  }

  func completeAddMediaBackground(
    input: RadrootsNativeUploadCompletion
  ) async throws -> RadrootsDraftStatus {
    try await addOperation("runtime.add.media.background.complete") { backend in
      try await backend.completeAddMediaBackground(input: input)
    }
  }

  func probeBlossom() async throws -> RadrootsBlossomEvidence {
    try await supportOperation("runtime.blossom.probe") { backend in
      try await backend.probeBlossom()
    }
  }

  func mobileSettings() async throws -> RadrootsMobileSettings {
    try await supportOperation("runtime.settings.read") { backend in
      try await backend.mobileSettings()
    }
  }

  func replaceMobileSettings(
    input: RadrootsReplaceSettings
  ) async throws -> RadrootsSettingsTransition {
    try await supportOperation("runtime.settings.replace") { backend in
      try await backend.replaceMobileSettings(input: input)
    }
  }

  func applyIdentityCommand(
    expectedRevision: UInt64,
    command: RadrootsIdentityCommand
  ) async throws -> RadrootsSettingsTransition {
    try await supportOperation("runtime.settings.identity") { backend in
      try await backend.applyIdentityCommand(
        expectedRevision: expectedRevision,
        command: command
      )
    }
  }

  func saveProfileMetadata(input: RadrootsProfileMetadataInput) async throws
    -> RadrootsProfileStatus
  {
    try await supportOperation("runtime.profile.save") { backend in
      try await backend.saveProfileMetadata(input: input)
    }
  }

  func profileStatus(operationID: String) async throws -> RadrootsProfileStatus {
    try await supportOperation("runtime.profile.status") { backend in
      try await backend.profileStatus(operationID: operationID)
    }
  }

  func advanceProfile(operationID: String) async throws -> RadrootsProfileStatus {
    try await supportOperation("runtime.profile.advance") { backend in
      try await backend.advanceProfile(operationID: operationID)
    }
  }

  func cancelProfile(
    operationID: String,
    expectedRevision: UInt64
  ) async throws -> RadrootsProfileStatus {
    try await supportOperation("runtime.profile.cancel") { backend in
      try await backend.cancelProfile(
        operationID: operationID,
        expectedRevision: expectedRevision
      )
    }
  }

  func advanceDraft(id: String, expectedRevision: UInt64) async throws -> RadrootsDraftStatus {
    try await addOperation("runtime.add.advance") { backend in
      try await backend.advanceDraft(id: id, expectedRevision: expectedRevision)
    }
  }

  func cancelAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> RadrootsDraftStatus {
    try await addOperation("runtime.add.cancel") { backend in
      try await backend.cancelAddIntent(
        id: id,
        expectedRevision: expectedRevision
      )
    }
  }

  func changes(bufferCapacity: Int = 16) async throws -> AsyncStream<RadrootsRuntimeChange> {
    guard (1 ... 64).contains(bufferCapacity) else {
      throw RadrootsRuntimeClientError.invalidBufferCapacity
    }
    guard let backend, case .running = lifecycleState else {
      throw RadrootsRuntimeClientError.notRunning
    }

    let id = UUID()
    let subscriptionGeneration = generation
    let identity = nextIdentity(kind: .subscription)
    let pair = AsyncStream.makeStream(
      of: RadrootsRuntimeChange.self,
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
    let task = RadrootsRuntimeBoundedTask<any RadrootsRuntimeSubscriptionToken>(
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
        let cancellation = RadrootsRuntimeBoundedTask<Void>(
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
      throw RadrootsRuntimeClientError.superseded
    }

    switch outcome {
    case let .completed(.success(token)):
      subscription.token = token
      subscriptions[id] = subscription
      return pair.stream
    case let .completed(.failure(failure)):
      subscriptions.removeValue(forKey: id)?.continuation.finish()
      throw RadrootsRuntimeClientError.subscription(failure)
    case .timedOut:
      subscriptions.removeValue(forKey: id)?.continuation.finish()
      throw RadrootsRuntimeClientError.subscription(
        Self.deadlineFailure(identity: identity)
      )
    case .cancelled:
      subscriptions.removeValue(forKey: id)?.continuation.finish()
      throw RadrootsRuntimeClientError.subscription(
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
      generation &+= 1
      startupOperation.task.cancel()
      self.startupOperation = nil
      configuration = nil
      lifecycleState = .stopped
    }
  }

  func stop() async throws -> RadrootsRuntimeShutdownReceipt {
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
  ) async throws -> RadrootsRuntimeSnapshot {
    let outcome = await operation.task.value(cancelsOperationWhenWaiterCancelled: false)
    guard generation == operation.identity.generation else {
      throw RadrootsRuntimeClientError.superseded
    }

    if case .cancelled = outcome,
       startupOperation?.identity == operation.identity
    {
      throw RadrootsRuntimeClientError.startup(
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
      throw RadrootsRuntimeClientError.superseded
    }

    switch outcome {
    case let .completed(.success(started)):
      backend = started.backend
      configuration = operation.configuration
      lifecycleState = .running(generation: operation.identity.generation)
      return started.snapshot
    case let .completed(.failure(failure)):
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw RadrootsRuntimeClientError.startup(failure)
    case .timedOut:
      let failure = Self.deadlineFailure(identity: operation.identity)
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw RadrootsRuntimeClientError.startup(failure)
    case .cancelled:
      let failure = Self.cancellationFailure(identity: operation.identity)
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw RadrootsRuntimeClientError.startup(failure)
    }
  }

  private func beginShutdown() -> ShutdownOperation {
    generation &+= 1
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
    let task = RadrootsRuntimeBoundedTask<RadrootsRuntimeShutdownReceipt>(
      deadlineNanoseconds: deadlines.shutdownNanoseconds,
      operation: {
        let cancellations = activeSubscriptions.compactMap(\.token).map { token in
          RadrootsRuntimeBoundedTask<Void>(
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
  ) async throws -> RadrootsRuntimeShutdownReceipt {
    let outcome = await operation.task.value(cancelsOperationWhenWaiterCancelled: false)
    if case .cancelled = outcome,
       shutdownOperation?.identity == operation.identity,
       generation == operation.identity.generation
    {
      throw RadrootsRuntimeClientError.shutdown(
        Self.cancellationFailure(identity: operation.identity)
      )
    }
    if shutdownOperation?.identity == operation.identity {
      shutdownOperation = nil
    }

    let lateSucceeded = lateShutdownSuccesses.remove(operation.identity) != nil
    guard generation == operation.identity.generation else {
      throw RadrootsRuntimeClientError.superseded
    }

    switch outcome {
    case let .completed(.success(receipt)):
      quarantinedBackend = nil
      lifecycleState = .stopped
      return receipt
    case let .completed(.failure(failure)):
      quarantinedBackend = operation.backend
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw RadrootsRuntimeClientError.shutdown(failure)
    case .timedOut:
      let failure = Self.deadlineFailure(identity: operation.identity)
      quarantinedBackend = lateSucceeded ? nil : operation.backend
      lifecycleState =
        lateSucceeded
          ? .stopped
          : .failed(generation: operation.identity.generation, failure: failure)
      throw RadrootsRuntimeClientError.shutdown(failure)
    case .cancelled:
      let failure = Self.cancellationFailure(identity: operation.identity)
      quarantinedBackend = lateSucceeded ? nil : operation.backend
      lifecycleState =
        lateSucceeded
          ? .stopped
          : .failed(generation: operation.identity.generation, failure: failure)
      throw RadrootsRuntimeClientError.shutdown(failure)
    }
  }

  private func runtimeOperation<T: Sendable>(
    _ operation: String,
    _ body: @escaping @Sendable (any RadrootsRuntimeBackend) async throws -> T
  ) async throws -> T {
    guard let backend, case .running = lifecycleState else {
      throw RadrootsRuntimeClientError.notRunning
    }
    let operationGeneration = generation
    let identity = nextIdentity(kind: .operation)
    let task = RadrootsRuntimeBoundedTask<T>(
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
      throw RadrootsRuntimeClientError.superseded
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
    _ body: @escaping @Sendable (any RadrootsRuntimeBackend) async throws -> T
  ) async throws -> T {
    do {
      return try await runtimeOperation(operation, body)
    } catch let error as RadrootsRuntimeClientError {
      throw error
    } catch {
      throw RadrootsRuntimeClientError.add(
        Self.failure(from: error, operation: operation)
      )
    }
  }

  private func supportOperation<T: Sendable>(
    _ operation: String,
    _ body: @escaping @Sendable (any RadrootsRuntimeBackend) async throws -> T
  ) async throws -> T {
    do {
      return try await runtimeOperation(operation, body)
    } catch let error as RadrootsRuntimeClientError {
      throw error
    } catch {
      throw RadrootsRuntimeClientError.support(
        Self.failure(from: error, operation: operation)
      )
    }
  }

  private func receive(
    _ change: RadrootsRuntimeChange,
    subscriptionID: UUID,
    generation subscriptionGeneration: UInt64
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

  private func cancelSubscription(id: UUID, generation subscriptionGeneration: UInt64) {
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

  private func cancelTokenDetached(_ token: any RadrootsRuntimeSubscriptionToken) {
    let deadline = deadlines.subscriptionNanoseconds
    Task {
      let cancellation = RadrootsRuntimeBoundedTask<Void>(
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
    identity: RadrootsRuntimeOperationIdentity,
    result: Result<RadrootsRuntimeShutdownReceipt, RadrootsRuntimeFailure>
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
    kind: RadrootsRuntimeOperationKind
  ) -> RadrootsRuntimeOperationIdentity {
    operationSequence &+= 1
    return RadrootsRuntimeOperationIdentity(
      generation: generation,
      sequence: operationSequence,
      kind: kind
    )
  }

  private func removeActiveOperation(_ identity: RadrootsRuntimeOperationIdentity) {
    guard activeOperations[identity.sequence]?.identity == identity else { return }
    activeOperations.removeValue(forKey: identity.sequence)
  }

  private static func deadlineFailure(
    identity: RadrootsRuntimeOperationIdentity
  ) -> RadrootsRuntimeFailure {
    .local(
      operation: identity.rawValue,
      code: "ios.runtime.deadline_exceeded",
      safeMessage: "The Radroots runtime operation did not finish in time."
    )
  }

  private static func cancellationFailure(
    identity: RadrootsRuntimeOperationIdentity
  ) -> RadrootsRuntimeFailure {
    .local(
      operation: identity.rawValue,
      code: "ios.runtime.cancelled",
      safeMessage: "The Radroots runtime operation was cancelled."
    )
  }

  private static func failure(from error: Error, operation: String) -> RadrootsRuntimeFailure {
    if let failure = error as? RadrootsRuntimeFailure {
      return failure
    }
    if error is CancellationError {
      return .local(
        operation: operation,
        code: "ios.runtime.cancelled",
        safeMessage: "The Radroots runtime operation was cancelled."
      )
    }
    if case let RadrootsRuntimeClientError.startup(failure) = error {
      return failure
    }
    if case let RadrootsRuntimeClientError.subscription(failure) = error {
      return failure
    }
    if case let RadrootsRuntimeClientError.status(failure) = error {
      return failure
    }
    if case let RadrootsRuntimeClientError.today(failure) = error {
      return failure
    }
    if case let RadrootsRuntimeClientError.add(failure) = error {
      return failure
    }
    if case let RadrootsRuntimeClientError.support(failure) = error {
      return failure
    }
    if case let RadrootsRuntimeClientError.shutdown(failure) = error {
      return failure
    }
    return .local(
      operation: operation,
      code: "ios.runtime.unexpected",
      safeMessage: "The Radroots runtime could not complete the operation."
    )
  }
}
