import Foundation

/// Caller-owned bounded recovery; advisory status never grants effect authority.
@MainActor
final class TeraNativeRepairStore: ObservableObject {
  static let previewLimit = 64
  @Published private(set) var progress: TeraNativeRecoveryProgress?
  @Published private(set) var issues: [TeraNativeRecoveryIssue] = []
  @Published private(set) var message: String?
  @Published private(set) var isRunning = false
  private let client: TeraRuntimeClient
  private let media: (any TeraAddMediaHandling)?
  private var author: String?
  private var generation = TeraSessionGeneration.initial
  private var task: Task<Void, Never>?

  init(client: TeraRuntimeClient, media: (any TeraAddMediaHandling)?) {
    self.client = client
    self.media = media
  }

  deinit { task?.cancel() }

  func configure(author: String) {
    guard self.author != author else { return }
    stop()
    self.author = author
    progress = nil
    issues = []
    message = nil
  }

  func stop() {
    generation = generation.invalidated()
    task?.cancel()
  }

  func retry() {
    guard task == nil, !isRunning else { return }
    task = Task { [weak self] in
      guard let self else { return }
      _ = await reconcile()
      task = nil
    }
  }

  @discardableResult
  func reconcile() async -> String? {
    guard !isRunning, !Task.isCancelled else { return message }
    isRunning = true
    defer { isRunning = false }
    let requested = generation
    do {
      let result = try await media?.recoverNativeUploads(client: client)
      guard requested == generation, !Task.isCancelled else { return nil }
      let refreshed = await Self.refresh(issues, incoming: result?.issues ?? [], client: client)
      guard requested == generation, !Task.isCancelled else { return nil }
      issues = refreshed
      progress = result
      message = Self.message(result, retained: !issues.isEmpty)
    } catch {
      guard requested == generation, !Task.isCancelled else { return nil }
      let pause = TeraNativeRecoveryClassification.pause(error)
      progress = .init(visited: 0, remaining: 0, needsAttention: pause == nil, pause: pause)
      message = Self.message(progress, retained: !issues.isEmpty)
    }
    return message
  }

  private static func refresh(_ previous: [TeraNativeRecoveryIssue], incoming: [TeraNativeRecoveryIssue],
                              client: TeraRuntimeClient) async -> [TeraNativeRecoveryIssue]
  {
    var values: [String: TeraNativeRecoveryIssue] = [:]
    for issue in previous + incoming where issue.reason != .resolved {
      if values.count < previewLimit || values[issue.key] != nil {
        values[issue.key] = issue
      }
    }
    for key in values.keys.sorted() {
      if Task.isCancelled {
        break
      }
      guard let status = try? await client.nativeRecoveryStatus(key: key) else { continue }
      if status.reason == .resolved {
        values.removeValue(forKey: key)
      } else {
        values[key] = .init(key: key, reason: status.reason, status: status)
      }
    }
    return values.values.sorted { $0.key < $1.key }
  }

  private static func message(_ progress: TeraNativeRecoveryProgress?, retained: Bool) -> String? {
    guard let progress else { return nil }
    switch progress.pause {
    case .protectedData: return "Unlock the device, then check saved photos again. Saved editing is still available."
    case .storageUnavailable: return "Photo recovery is paused until local storage is available. Saved editing is still available."
    case nil: break
    }
    if progress.needsAttention || retained {
      return "Photo recovery needs attention. Saved editing is still available."
    }
    if progress.remaining > 0 {
      return "More saved photo recovery remains. Saved editing is still available."
    }
    return nil
  }
}
