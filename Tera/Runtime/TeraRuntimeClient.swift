import Foundation

actor TeraRuntimeClient {
  private struct StartupOperation: Sendable {
    let identity: TeraRuntimeOperationIdentity
    let configuration: TeraRuntimeLaunchConfiguration
    let task: TeraRuntimeResourceTask<TeraRuntimeBackendStart>
    var waiters = 0
  }

  private struct ShutdownOperation: Sendable {
    let identity: TeraRuntimeOperationIdentity
    let work: TeraRuntimeShutdownWork
    let task: TeraRuntimeShutdownTask
  }

  private struct ActiveOperation: Sendable {
    let identity: TeraRuntimeOperationIdentity
    let cancel: @Sendable () -> Void
    let drain: @Sendable () async throws -> Void
  }

  private let factory: TeraRuntimeBackendFactory
  private let deadlines: TeraRuntimeDeadlinePolicy
  private var generation = TeraSessionGeneration.initial
  private var operationSequence: UInt64 = 0
  private var lifecycleState: TeraRuntimeLifecycle = .stopped
  private var configuration: TeraRuntimeLaunchConfiguration?
  private var backend: (any TeraRuntimeBackend)?
  private var retryShutdownWork: TeraRuntimeShutdownWork?
  private var abandonedStartups: [UInt64: TeraRuntimeResourceTask<TeraRuntimeBackendStart>] = [:]
  private var startupOperation: StartupOperation?
  private var shutdownOperation: ShutdownOperation?
  private var activeOperations: [UInt64: ActiveOperation] = [:]
  private var subscriptions: [UUID: TeraRuntimeSubscription] = [:]

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

    if retryShutdownWork != nil || !abandonedStartups.isEmpty {
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
    let task = TeraRuntimeResourceCreation.startup(
      configuration: requestedConfiguration, factory: factory,
      identity: identity, deadlines: deadlines
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
    subscriptions[id] = TeraRuntimeSubscription(
      generation: subscriptionGeneration,
      continuation: pair.continuation,
      token: nil
    )

    let task = TeraRuntimeResourceCreation.subscription(
      backend: backend, bufferCapacity: bufferCapacity,
      identity: identity, deadline: deadlines.subscriptionNanoseconds
    ) { [weak self] change in
      await self?.receive(change, subscriptionID: id, generation: subscriptionGeneration)
    }
    trackCreation(task, identity: identity)
    let outcome = await task.value()
    if case .completed = outcome {
      removeActiveOperation(identity)
    }

    guard generation == subscriptionGeneration,
          case .running = lifecycleState,
          var subscription = subscriptions[id]
    else {
      subscriptions.removeValue(forKey: id)?.continuation.finish()
      task.cancel()
      throw TeraRuntimeClientError.superseded
    }

    if Task.isCancelled {
      subscriptions.removeValue(forKey: id)?.continuation.finish()
      task.cancel()
      throw TeraRuntimeClientError.subscription(Self.cancellationFailure(identity: identity))
    }

    switch outcome {
    case let .completed(.success(token)):
      guard task.adopt() else {
        subscriptions.removeValue(forKey: id)?.continuation.finish()
        throw TeraRuntimeClientError.superseded
      }
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
      abandonStartup(startupOperation)
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
      startupOperation != nil || backend != nil || retryShutdownWork != nil || !abandonedStartups.isEmpty
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
    if startupOperation?.identity == operation.identity {
      startupOperation?.waiters += 1
    }
    defer { releaseStartupWaiter(operation) }
    let outcome = await operation.task.value(cancelsOperationWhenWaiterCancelled: false)
    guard generation == operation.identity.generation else {
      operation.task.cancel()
      throw TeraRuntimeClientError.superseded
    }

    if Task.isCancelled {
      throw TeraRuntimeClientError.startup(Self.cancellationFailure(identity: operation.identity))
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
      guard operation.task.adopt() else { throw TeraRuntimeClientError.superseded }
      backend = started.backend
      configuration = operation.configuration
      lifecycleState = .running(generation: operation.identity.generation)
      return started.snapshot
    case let .completed(.failure(failure)):
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw TeraRuntimeClientError.startup(failure)
    case .timedOut:
      abandonStartup(operation)
      let failure = Self.deadlineFailure(identity: operation.identity)
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw TeraRuntimeClientError.startup(failure)
    case .cancelled:
      abandonStartup(operation)
      let failure = Self.cancellationFailure(identity: operation.identity)
      lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      throw TeraRuntimeClientError.startup(failure)
    }
  }

  private func releaseStartupWaiter(_ operation: StartupOperation) {
    guard var pending = startupOperation, pending.identity == operation.identity else { return }
    pending.waiters -= 1
    guard pending.waiters == 0 else {
      startupOperation = pending
      return
    }
    abandonStartup(pending)
    startupOperation = nil
    configuration = nil
    lifecycleState = .stopped
  }

  private func trackCreation(
    _ task: TeraRuntimeResourceTask<some Sendable>, identity: TeraRuntimeOperationIdentity
  ) {
    activeOperations[identity.sequence] = ActiveOperation(
      identity: identity,
      cancel: { task.cancel() },
      drain: { try await task.finishAbandonment() }
    )
  }

  private func abandonStartup(_ operation: StartupOperation) {
    operation.task.cancel()
    abandonedStartups[operation.identity.sequence] = operation.task
  }

  private func beginShutdown() -> ShutdownOperation {
    generation = generation.invalidated()
    if let startupOperation {
      abandonStartup(startupOperation)
    }
    let activeSubscriptions = Array(subscriptions.values)
    let activeRuntimeOperations = Array(activeOperations.values)
    let startups = Array(abandonedStartups.values)
    let activeBackend = backend
    startupOperation = nil
    backend = nil
    configuration = nil
    subscriptions.removeAll()
    activeOperations.removeAll()
    abandonedStartups.removeAll()
    lifecycleState = .stopping(generation: generation)

    for operation in activeRuntimeOperations {
      operation.cancel()
    }
    for subscription in activeSubscriptions {
      subscription.continuation.finish()
    }
    let drains: [@Sendable () async throws -> Void] =
      startups.map { startup in { @Sendable in try await startup.finishAbandonment() } }
      + activeRuntimeOperations.map(\.drain)
      + activeSubscriptions.compactMap(\.token).map { token in { @Sendable in await token.cancel() } }
    let work = retryShutdownWork ?? TeraRuntimeShutdownWork(backend: activeBackend, drains: drains)
    retryShutdownWork = work
    let identity = nextIdentity(kind: .shutdown)
    let task = TeraRuntimeShutdownTask(work: work, deadline: deadlines.shutdownNanoseconds) { [weak self] result in
      await self?.settleShutdown(identity: identity, result: result)
    }
    let operation = ShutdownOperation(identity: identity, work: work, task: task)
    shutdownOperation = operation
    return operation
  }

  private func finishShutdown(_ operation: ShutdownOperation) async throws -> TeraRuntimeShutdownReceipt {
    let outcome = await operation.task.value()
    guard generation == operation.identity.generation else { throw TeraRuntimeClientError.superseded }
    switch outcome {
    case let .completed(.success(receipt)):
      return receipt
    case let .completed(.failure(failure)):
      throw TeraRuntimeClientError.shutdown(failure)
    case .timedOut:
      let failure = Self.deadlineFailure(identity: operation.identity)
      if shutdownOperation?.identity == operation.identity {
        lifecycleState = .failed(generation: operation.identity.generation, failure: failure)
      }
      throw TeraRuntimeClientError.shutdown(failure)
    case .cancelled:
      throw TeraRuntimeClientError.shutdown(Self.cancellationFailure(identity: operation.identity))
    }
  }

  func runtimeOperation<T: Sendable>(
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
      operation: { [weak self] in
        let result: Result<T, TeraRuntimeFailure>
        do {
          result = try await .success(body(backend))
        } catch {
          result = .failure(Self.failure(from: error, operation: operation))
        }
        await self?.removeActiveOperation(identity)
        return result
      }
    )
    activeOperations[identity.sequence] = ActiveOperation(
      identity: identity,
      cancel: { task.cancel() },
      drain: { _ = await task.settle() }
    )
    let outcome = await task.value()

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
          var subscription = subscriptions[subscriptionID],
          subscription.generation == subscriptionGeneration,
          change.matches(configuration), subscription.admission.accept(change)
    else {
      return
    }
    subscriptions[subscriptionID] = subscription
    change.yield(to: subscription.continuation)
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
    let identity = nextIdentity(kind: .subscription)
    let task = TeraRuntimeBoundedTask<Void>(deadlineNanoseconds: deadlines.subscriptionNanoseconds) { [weak self] in
      await token.cancel()
      await self?.removeActiveOperation(identity)
      return .success(())
    }
    activeOperations[identity.sequence] = ActiveOperation(
      identity: identity, cancel: { task.cancel() }, drain: { _ = await task.settle() }
    )
  }

  private func settleShutdown(
    identity: TeraRuntimeOperationIdentity,
    result: Result<TeraRuntimeShutdownReceipt, TeraRuntimeFailure>
  ) {
    guard generation == identity.generation, shutdownOperation?.identity == identity else { return }
    shutdownOperation = nil
    switch result {
    case .success:
      retryShutdownWork = nil
      lifecycleState = .stopped
    case let .failure(failure):
      lifecycleState = .failed(generation: identity.generation, failure: failure)
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

  static func failure(from error: Error, operation: String) -> TeraRuntimeFailure {
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
