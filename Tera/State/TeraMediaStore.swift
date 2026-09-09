import Foundation
import UIKit

enum TeraMediaPresentationState: Equatable {
  case pending
  case loading
  case ready(TeraVerifiedMediaArtifact)
  case unavailable
  case networkUnavailable
  case corrupt
  case failed

  var accessibilityLabel: String {
    switch self {
    case .pending: "Photo verification is pending"
    case .loading: "Loading verified photo"
    case .ready: "Verified photo"
    case .unavailable: "Photo is not available locally"
    case .networkUnavailable: "The photo service could not be reached"
    case .corrupt: "The saved photo failed verification"
    case .failed: "Photo could not be loaded"
    }
  }
}

@MainActor
final class TeraMediaStore: ObservableObject {
  struct Request: Hashable {
    let referenceID: String
    let context: TeraLocalNetwork?
  }

  private struct Key: Hashable {
    let account: String?
    let context: TeraLocalNetwork
    let reference: String
  }

  private struct Work {
    let id: UUID
    let task: Task<Void, Never>
  }

  @Published private var states: [Key: TeraMediaPresentationState] = [:]

  private let runtimeClient: TeraRuntimeClient
  private var tasks: [Key: Work] = [:]
  private var configuration: TeraPresentationConfiguration?

  init(runtimeClient: TeraRuntimeClient) {
    self.runtimeClient = runtimeClient
  }

  deinit {
    for task in tasks.values {
      task.task.cancel()
    }
  }

  func state(
    for media: TeraMediaReference,
    context: TeraLocalNetwork?
  ) -> TeraMediaPresentationState {
    guard let context else { return .unavailable }
    if let state = states[key(media: media, context: context)] {
      return state
    }
    switch media.verification {
    case .pending: return .pending
    case .failed: return .failed
    case .verified, .unavailable: return .unavailable
    }
  }

  func load(media: TeraMediaReference, context: TeraLocalNetwork?) {
    guard let context else { return }
    let key = key(media: media, context: context)
    guard states[key] == nil, tasks[key] == nil else { return }
    switch media.verification {
    case .pending:
      states[key] = .pending
    case .failed:
      states[key] = .failed
    case .verified:
      guard let artifactID = media.verifiedArtifactID else {
        states[key] = .corrupt
        return
      }
      start(key: key, context: context) { [runtimeClient] in
        try await runtimeClient.verifiedMediaArtifact(
          context: context,
          artifactID: artifactID
        )
      }
    case .unavailable:
      start(key: key, context: context) { [runtimeClient] in
        try await runtimeClient.retrieveMedia(context: context, reference: media)
      }
    }
  }

  func retry(media: TeraMediaReference, context: TeraLocalNetwork?) {
    guard let context else { return }
    let key = key(media: media, context: context)
    tasks[key]?.task.cancel()
    tasks[key] = nil
    states[key] = nil
    start(key: key, context: context) { [runtimeClient] in
      try await runtimeClient.retrieveMedia(context: context, reference: media)
    }
  }

  func reset() {
    for task in tasks.values {
      task.task.cancel()
    }
    tasks.removeAll(keepingCapacity: false)
    states.removeAll(keepingCapacity: false)
  }

  func configure(snapshot: TeraRuntimeSnapshot) {
    let updated = TeraPresentationConfiguration(snapshot: snapshot)
    guard configuration != updated else { return }
    reset()
    configuration = updated
  }

  private func start(
    key: Key,
    context: TeraLocalNetwork,
    operation: @escaping @Sendable () async throws -> TeraVerifiedMediaArtifact?
  ) {
    states[key] = .loading
    let id = UUID()
    let task = Task { [weak self] in
      do {
        let artifact = try await operation()
        guard let self, isCurrent(key: key, id: id) else { return }
        guard let artifact else {
          complete(key: key, id: id, state: .unavailable)
          return
        }
        guard UIImage(data: artifact.bytes) != nil else {
          _ = try? await runtimeClient.invalidateMediaArtifact(
            context: context, artifactID: artifact.artifactID
          )
          complete(key: key, id: id, state: .corrupt)
          return
        }
        complete(key: key, id: id, state: .ready(artifact))
      } catch is CancellationError {
        self?.complete(key: key, id: id, state: nil)
      } catch {
        self?.complete(key: key, id: id, state: Self.failureState(error))
      }
    }
    tasks[key] = Work(id: id, task: task)
  }

  private func isCurrent(key: Key, id: UUID) -> Bool {
    tasks[key]?.id == id && !Task.isCancelled
  }

  private func complete(key: Key, id: UUID, state: TeraMediaPresentationState?) {
    guard tasks[key]?.id == id else { return }
    tasks[key] = nil
    states[key] = Task.isCancelled ? nil : state
  }

  private func key(media: TeraMediaReference, context: TeraLocalNetwork) -> Key {
    Key(account: configuration?.publicKey, context: context, reference: media.referenceFingerprint)
  }

  static func failureState(_ error: Error) -> TeraMediaPresentationState {
    guard let failure = TeraRuntimeFailure.from(error) else { return .failed }
    if failure.recovery.disposition == .mediaCorrupt {
      return .corrupt
    }
    if failure.recovery.disposition == .networkUnavailable {
      return .networkUnavailable
    }
    return .failed
  }
}
