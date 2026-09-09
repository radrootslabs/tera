import Foundation

/// Stable configuration inputs, excluding changing service observation evidence.
struct TeraPresentationConfiguration: Equatable {
  let publicKey: String
  let context: TeraLocalNetwork
  let blossom: TeraBlossomConfigurationStatus?

  init(snapshot: TeraRuntimeSnapshot) {
    publicKey = snapshot.identity.publicKeyHex
    context = .defaultContext(snapshot: snapshot)
    blossom = snapshot.blossomConfiguration
  }
}

/// At most one entry per fixed notification domain; a gap subsumes every domain.
struct TeraObservationBatch: Equatable {
  private(set) var domains: Set<TeraRuntimeChangeKind> = []
  private(set) var resnapshot = false

  static var currentState: Self {
    Self(resnapshot: true)
  }

  var isEmpty: Bool {
    !resnapshot && domains.isEmpty
  }

  func contains(anyOf kinds: Set<TeraRuntimeChangeKind>) -> Bool {
    resnapshot || !domains.isDisjoint(with: kinds)
  }

  mutating func insert(_ change: TeraRuntimeChange) {
    if change.delivery == .resnapshotRequired || change.revision.rawValue == nil {
      self = .currentState
    } else if !resnapshot, change.kind != .initial {
      domains.insert(change.kind)
    }
  }
}

/// One stream and one refresh worker per store. Ingestion never waits for a
/// query; hints arriving during that query retain a bounded final refresh.
@MainActor
final class TeraStoreObservation {
  private var generation = TeraSessionGeneration.initial
  private var task: Task<Void, Never>?
  private var refreshTask: Task<Void, Never>?
  private var pending = TeraObservationBatch()

  var isActive: Bool {
    task != nil
  }

  deinit {
    task?.cancel()
    refreshTask?.cancel()
  }

  func stop() {
    generation = generation.invalidated()
    task?.cancel()
    task = nil
    refreshTask?.cancel()
    refreshTask = nil
    pending = TeraObservationBatch()
  }

  func start(
    client: TeraRuntimeClient,
    buffer: (capacity: Int, delay: @Sendable (UInt32) async throws -> Void),
    state: @escaping @MainActor (TeraRuntimeObservationState) -> Void,
    accepts: @escaping @MainActor (TeraRuntimeChange) -> Bool,
    refresh: @escaping @MainActor (TeraObservationBatch) async -> Void
  ) {
    guard task == nil, generation.isActive else { return }
    generation = generation.invalidated()
    guard generation.isActive else { return }
    let requested = generation
    task = Task { [weak self] in
      var attempt: UInt32 = 0
      while self?.isCurrent(requested) == true {
        state(.subscribing(attempt: attempt == .max ? .max : attempt + 1))
        var message = TeraUserMessages.text(.runtimeObservationUnavailable)
        do {
          let changes = try await client.changes(bufferCapacity: buffer.capacity)
          guard self?.isCurrent(requested) == true else { break }
          state(.active)
          self?.requireCurrentState(requested, refresh: refresh)
          for await value in changes {
            guard self?.isCurrent(requested) == true else { break }
            attempt = 0
            if accepts(value) {
              self?.pending.insert(value)
              self?.startRefresh(requested, refresh: refresh)
            }
          }
        } catch {
          message = TeraUserMessages.text(for: error, fallback: .runtimeObservationUnavailable)
        }
        guard self?.isCurrent(requested) == true else { break }
        attempt = attempt == .max ? .max : attempt + 1
        state(.retrying(attempt: attempt, message: message))
        do { try await buffer.delay(attempt) } catch { break }
      }
      self?.finish(requested, state: state)
    }
  }

  private func requireCurrentState(
    _ requested: TeraSessionGeneration,
    refresh: @escaping @MainActor (TeraObservationBatch) async -> Void
  ) {
    pending = .currentState
    startRefresh(requested, refresh: refresh)
  }

  private func startRefresh(
    _ requested: TeraSessionGeneration,
    refresh: @escaping @MainActor (TeraObservationBatch) async -> Void
  ) {
    guard refreshTask == nil, !pending.isEmpty, isCurrent(requested) else { return }
    refreshTask = Task { [weak self] in
      while let batch = self?.takePending(requested) {
        await refresh(batch)
      }
      guard self?.generation == requested else { return }
      self?.refreshTask = nil
    }
  }

  private func takePending(_ requested: TeraSessionGeneration) -> TeraObservationBatch? {
    guard isCurrent(requested), !pending.isEmpty else { return nil }
    let batch = pending
    pending = TeraObservationBatch()
    return batch
  }

  private func finish(
    _ requested: TeraSessionGeneration, state: @MainActor (TeraRuntimeObservationState) -> Void
  ) {
    guard generation == requested else { return }
    let cancelled = Task.isCancelled
    stop()
    if cancelled {
      state(.stopped)
    }
  }

  private func isCurrent(_ requested: TeraSessionGeneration) -> Bool {
    generation == requested && generation.isActive && !Task.isCancelled
  }
}
