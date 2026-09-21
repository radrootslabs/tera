import Foundation

/// Caller-owned bounded recovery; advisory status never grants effect authority.
@MainActor
final class TeraNativeRepairStore: ObservableObject {
  static let previewLimit = 64
  static let batchLimit = 4
  @Published private(set) var progress: TeraNativeRecoveryProgress?
  @Published private(set) var issues: [TeraNativeRecoveryIssue] = []
  @Published private(set) var message: String?
  @Published private(set) var isRunning = false
  private let client: TeraRuntimeClient
  private let media: (any TeraAddMediaHandling)?
  private var author: String?
  private var generation = TeraSessionGeneration.initial
  private var task: Task<Void, Never>?
  private enum Request { case sweep, transfer(String) }
  private var pending: Request?

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
    pending = nil
    task?.cancel()
  }

  func retry() {
    enqueue(.sweep)
  }

  func check(_ issue: TeraNativeRecoveryIssue) {
    guard issues.contains(where: { $0.key == issue.key }) else { return }
    enqueue(.transfer(issue.key))
  }

  private func enqueue(_ request: Request) {
    guard !Task.isCancelled, generation.isActive else { return }
    // Coalesce live requests. A request after stop is retained until the old
    // worker actually returns, so cancellation cannot release its ownership.
    guard task == nil || task?.isCancelled == true else { return }
    pending = request
    startWorker()
  }

  private func startWorker() {
    guard task == nil, let request = pending, generation.isActive else { return }
    pending = nil
    let requested = generation
    isRunning = true
    task = Task { [weak self] in
      guard let self else { return }
      await run(requested, request: request)
      isRunning = false
      task = nil
      startWorker()
    }
  }

  @discardableResult
  func reconcile() async -> String? {
    guard !Task.isCancelled else { return message }
    retry()
    guard let task else { return message }
    await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
    return message
  }

  /// Foreground resumes must recover even when the event observer is already
  /// live; neither a new draft nor a native transfer callback is required.
  func resumeIfObserving(_ observing: Bool) async -> Bool {
    guard observing else { return false }
    await reconcile()
    return true
  }

  private func run(_ requested: TeraSessionGeneration, request: Request) async {
    for _ in 0 ..< Self.batchLimit {
      guard requested == generation, !Task.isCancelled else { return }
      await batch(requested, request: request)
      if case .transfer = request {
        return
      }
      guard requested == generation, !Task.isCancelled,
            let progress, progress.pause == nil, progress.remaining > 0, progress.visited > 0
      else { return }
      await Task.yield()
    }
  }

  private func batch(_ requested: TeraSessionGeneration, request: Request) async {
    do {
      let result: TeraNativeRecoveryProgress? = switch request {
      case .sweep: try await media?.recoverNativeUploads(client: client)
      case let .transfer(key): try await media?.recoverNativeUpload(key: key, client: client)
      }
      guard requested == generation, !Task.isCancelled else { return }
      if let result {
        guard (0 ... TeraNativeRecoveryInventory.passLimit).contains(result.visited), result.remaining >= 0,
              result.issues.count <= Self.previewLimit else { throw TeraComposerAcknowledgment.unconfirmed }
      }
      let refreshed = await Self.refresh(issues, incoming: result?.issues ?? [], client: client)
      guard requested == generation, !Task.isCancelled else { return }
      issues = refreshed
      if case .transfer = request, let result {
        // A selected check does not advance the persistent sweep cursor.
        progress = .init(visited: result.visited, remaining: progress?.remaining ?? 0,
                         needsAttention: result.needsAttention, issues: result.issues, pause: result.pause)
      } else {
        progress = result
      }
      message = Self.message(progress, retained: !issues.isEmpty)
    } catch {
      guard requested == generation, !Task.isCancelled else { return }
      let pause = TeraNativeRecoveryClassification.pause(error)
      progress = .init(visited: 0, remaining: progress?.remaining ?? 0, needsAttention: pause == nil, pause: pause)
      message = Self.message(progress, retained: !issues.isEmpty)
    }
  }

  private static func refresh(_ previous: [TeraNativeRecoveryIssue], incoming: [TeraNativeRecoveryIssue],
                              client: TeraRuntimeClient) async -> [TeraNativeRecoveryIssue]
  {
    var values: [String: TeraNativeRecoveryIssue] = [:]
    let resolved = Set(incoming.filter { $0.reason == .resolved }.map(\.key))
    for issue in previous + incoming where issue.reason != .resolved && !resolved.contains(issue.key) {
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
    if let pause = progress.pause {
      return pause.message
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
