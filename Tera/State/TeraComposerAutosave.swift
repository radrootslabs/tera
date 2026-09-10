import Foundation

/// A single worker retains one attempted write and the newest edit. Receipts
/// update acknowledgment metadata only; they never replace the editing form.
@MainActor
final class TeraComposerAutosave {
  private(set) var state: TeraComposerSaveState = .idle {
    didSet {
      if state != oldValue {
        stateChanged(state)
      }
    }
  }

  var stateChanged: (TeraComposerSaveState) -> Void = { _ in }
  private(set) var acknowledged: TeraComposerDraft?
  private(set) var scope: TeraComposerScope?
  private(set) var editSequence: UInt64 = 0
  private(set) var id: String?
  private let persistence: TeraComposerPersistence
  private let delay: @Sendable () async throws -> Void
  private var generation = TeraSessionGeneration.initial
  private var current: TeraComposerForm?
  private var attempted: TeraComposerSaveRequest?
  private var worker: Task<Void, Never>?
  private var paused = false
  private var exhausted = false

  init(
    persistence: TeraComposerPersistence,
    delay: @escaping @Sendable () async throws -> Void = { try await Task.sleep(for: .milliseconds(250)) }
  ) {
    self.persistence = persistence
    self.delay = delay
  }

  deinit { worker?.cancel() }

  var isDirty: Bool {
    current != nil && (exhausted || acknowledged?.editSequence != editSequence || acknowledged?.form != current)
  }

  /// A replaced scope or New operation invalidates callbacks, but keeps the
  /// worker slot occupied until it exits, preventing abandoned task fan-out.
  func reset(scope: TeraComposerScope?) {
    generation = generation.invalidated()
    worker?.cancel()
    self.scope = scope
    id = nil
    current = nil
    acknowledged = nil
    attempted = nil
    editSequence = 0
    exhausted = false
    paused = false
    state = .idle
  }

  func stop() {
    generation = generation.invalidated()
    paused = true
    worker?.cancel()
    if isDirty {
      state = .unsaved
    }
  }

  func resume() {
    paused = false
    startWorker()
  }

  func change(_ form: TeraComposerForm) {
    guard form != current else { return }
    current = form
    let (next, overflow) = editSequence.addingReportingOverflow(1)
    guard !overflow, !exhausted else {
      exhausted = true
      state = .failed
      return
    }
    editSequence = next
    if state != .failed {
      state = .unsaved
    }
    startWorker()
  }

  func save(_ form: TeraComposerForm) async throws -> TeraComposerDraft {
    guard !Task.isCancelled else { throw CancellationError() }
    change(form)
    guard !paused, !exhausted, generation.isActive, scope != nil else {
      state = .failed
      throw TeraComposerAcknowledgment.unconfirmed
    }
    let requested = generation
    if state == .failed {
      state = .unsaved
    }
    startWorker()
    while let task = worker {
      await task.value
      try ensureCurrent(requested)
    }
    guard !isDirty, let acknowledged else { throw TeraComposerAcknowledgment.unconfirmed }
    return acknowledged
  }

  private func startWorker() {
    guard worker == nil, !paused, !exhausted, generation.isActive, scope != nil,
          isDirty, state != .failed else { return }
    let requested = generation
    worker = Task { [weak self] in
      guard let self else { return }
      await run(requested)
    }
  }

  private func run(_ requested: TeraSessionGeneration) async {
    defer {
      worker = nil
      startWorker()
    }
    do {
      while isDirty {
        try ensureCurrent(requested)
        state = .saving
        try await delay()
        try ensureCurrent(requested)
        try await reconcileAttempt(requested)
        try await persistCurrent(requested)
      }
      try ensureCurrent(requested)
      state = .saved
    } catch {
      // A write may have committed before cancellation or a lost callback.
      // Keep its exact request for a read before any retry in this lifetime.
      if generation == requested, !paused {
        state = .failed
      }
    }
  }

  private func persistCurrent(_ requested: TeraSessionGeneration) async throws {
    if id == nil {
      let reserved = try await persistence.reserve()
      try ensureCurrent(requested)
      guard TeraAddPresentation.isValidIdentifier(reserved), reserved != String(repeating: "0", count: 32) else {
        throw TeraComposerAcknowledgment.unconfirmed
      }
      id = reserved
    }
    guard isDirty, let id, let scope, let current else { return }
    let request = TeraComposerSaveRequest(scope: scope, id: id, expectedRevision: acknowledged?.revision,
                                          editSequence: editSequence, form: current)
    attempted = request
    let receipt = try await persistence.save(request)
    try ensureCurrent(requested)
    guard TeraComposerAcknowledgment.matches(receipt.draft, request: request) else {
      throw TeraComposerAcknowledgment.unconfirmed
    }
    acknowledged = receipt.draft
    attempted = nil
  }

  private func reconcileAttempt(_ requested: TeraSessionGeneration) async throws {
    guard let attempted else { return }
    let loaded: TeraComposerDraft
    do {
      loaded = try await persistence.load(attempted.scope, attempted.id)
    } catch {
      try ensureCurrent(requested)
      if attempted.expectedRevision == nil,
         TeraAddPresentation.failure(for: error)?.code == "composer_not_found"
      {
        self.attempted = nil
        return
      }
      throw error
    }
    try ensureCurrent(requested)
    if TeraComposerAcknowledgment.matches(loaded, request: attempted) {
      acknowledged = loaded
    } else if loaded != acknowledged {
      // A different writer won the CAS. Preserve this form for recovery;
      // silently adopting its revision would overwrite someone else's edit.
      throw TeraComposerAcknowledgment.unconfirmed
    }
    self.attempted = nil
  }

  private func ensureCurrent(_ requested: TeraSessionGeneration) throws {
    guard generation == requested, generation.isActive, !paused, !exhausted, !Task.isCancelled else {
      throw CancellationError()
    }
  }
}
