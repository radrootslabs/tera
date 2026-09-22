import Foundation

/// Each action is explicit and bounded; no worker authorizes restored delivery.
@MainActor
final class TeraRestoreRecoveryStore: ObservableObject {
  @Published private(set) var status: TeraRestoreStatus?
  @Published private(set) var isWorking = false
  @Published private(set) var message: String?
  @Published private(set) var reviewedInventory: String?
  private let client: TeraRuntimeClient
  private var generation = TeraSessionGeneration.initial

  init(client: TeraRuntimeClient) {
    self.client = client
  }

  func invalidate() {
    generation = generation.invalidated()
    status = nil
    message = nil
    reviewedInventory = nil
  }

  func load() async {
    await perform(.load)
  }

  func checkNext() async {
    await perform(.check)
  }

  func review() async {
    await perform(.review)
  }

  func resume() async {
    await perform(.resume)
  }

  private enum Action { case load, check, review, resume }

  private func perform(_ action: Action) async {
    guard !isWorking, generation.isActive else { return }
    isWorking = true
    let selected = generation
    defer { isWorking = false }
    do {
      let digest = try await execute(action)
      let current = try await client.restoreStatus()
      guard generation == selected, !Task.isCancelled else { return }
      status = current
      reviewedInventory = digest
      message = nil
    } catch {
      guard generation == selected, !Task.isCancelled else { return }
      reviewedInventory = nil
      message = "The recovery check could not finish. Saved work remains on this device; check again before resuming."
    }
  }

  private func execute(_ action: Action) async throws -> String? {
    switch action {
    case .load: return nil
    case .check:
      guard let target = status?.targets.first(where: { $0.observation == nil })
        ?? status?.targets.first(where: { $0.observation == .incomplete }) else { return nil }
      try await client.reconcileRestoredTarget(target)
      return nil
    case .review: return try await client.reviewRestoredWork()
    case .resume:
      guard let reviewedInventory else { throw TeraComposerAcknowledgment.unconfirmed }
      try await client.resumeRestoredWork(reviewedInventory: reviewedInventory)
      return nil
    }
  }
}
