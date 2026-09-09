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

/// Each store owns one observer. An old subscription or waiter cannot update or
/// clear a replacement observer, including while subscription creation is pending.
@MainActor
final class TeraStoreObservation {
  private var generation = TeraSessionGeneration.initial
  private var task: Task<Void, Never>?

  var isActive: Bool {
    task != nil
  }

  deinit { task?.cancel() }

  func stop() {
    generation = generation.invalidated()
    task?.cancel()
    task = nil
  }

  func start(
    client: TeraRuntimeClient,
    capacity: Int,
    delay: @escaping @Sendable (UInt32) async throws -> Void,
    state: @escaping @MainActor (TeraRuntimeObservationState) -> Void,
    change: @escaping @MainActor (TeraRuntimeChange) async -> Void
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
          let changes = try await client.changes(bufferCapacity: capacity)
          guard self?.isCurrent(requested) == true else { break }
          state(.active)
          for await value in changes {
            guard self?.isCurrent(requested) == true else { break }
            attempt = 0
            await change(value)
          }
        } catch {
          message = TeraUserMessages.text(for: error, fallback: .runtimeObservationUnavailable)
        }
        guard self?.isCurrent(requested) == true else { break }
        attempt = attempt == .max ? .max : attempt + 1
        state(.retrying(attempt: attempt, message: message))
        do { try await delay(attempt) } catch { break }
      }
      self?.finish(requested, state: state)
    }
  }

  private func finish(
    _ requested: TeraSessionGeneration, state: @MainActor (TeraRuntimeObservationState) -> Void
  ) {
    guard generation == requested else { return }
    task = nil
    if Task.isCancelled {
      state(.stopped)
    }
  }

  private func isCurrent(_ requested: TeraSessionGeneration) -> Bool {
    generation == requested && generation.isActive && !Task.isCancelled
  }
}
