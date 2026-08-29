import Foundation
import UIKit

enum RadrootsMediaPresentationState: Equatable {
  case pending
  case loading
  case ready(RadrootsVerifiedMediaArtifact)
  case unavailable
  case offline
  case corrupt
  case failed

  var accessibilityLabel: String {
    switch self {
    case .pending: "Photo verification is pending"
    case .loading: "Loading verified photo"
    case .ready: "Verified photo"
    case .unavailable: "Photo is not available locally"
    case .offline: "Photo is unavailable while offline"
    case .corrupt: "The saved photo failed verification"
    case .failed: "Photo could not be loaded"
    }
  }
}

@MainActor
final class RadrootsMediaStore: ObservableObject {
  @Published private var states: [String: RadrootsMediaPresentationState] = [:]

  private let runtimeClient: RadrootsRuntimeClient
  private var tasks: [String: Task<Void, Never>] = [:]

  init(runtimeClient: RadrootsRuntimeClient) {
    self.runtimeClient = runtimeClient
  }

  deinit {
    for task in tasks.values {
      task.cancel()
    }
  }

  func state(
    for media: RadrootsMediaReference,
    context: RadrootsLocalNetwork?
  ) -> RadrootsMediaPresentationState {
    guard let context else { return .unavailable }
    if let state = states[Self.key(media: media, context: context)] {
      return state
    }
    switch media.verification {
    case .pending: return .pending
    case .failed: return .failed
    case .verified, .unavailable: return .unavailable
    }
  }

  func load(media: RadrootsMediaReference, context: RadrootsLocalNetwork?) {
    guard let context else { return }
    let key = Self.key(media: media, context: context)
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

  func retry(media: RadrootsMediaReference, context: RadrootsLocalNetwork?) {
    guard let context else { return }
    let key = Self.key(media: media, context: context)
    tasks[key]?.cancel()
    tasks[key] = nil
    states[key] = nil
    start(key: key, context: context) { [runtimeClient] in
      try await runtimeClient.retrieveMedia(context: context, reference: media)
    }
  }

  func reset() {
    for task in tasks.values {
      task.cancel()
    }
    tasks.removeAll(keepingCapacity: false)
    states.removeAll(keepingCapacity: false)
  }

  private func start(
    key: String,
    context: RadrootsLocalNetwork,
    operation: @escaping @Sendable () async throws -> RadrootsVerifiedMediaArtifact?
  ) {
    states[key] = .loading
    tasks[key] = Task { [weak self] in
      guard let self else { return }
      do {
        guard let artifact = try await operation() else {
          complete(key: key, state: .unavailable)
          return
        }
        guard UIImage(data: artifact.bytes) != nil else {
          await invalidateCorrupt(artifact: artifact, context: context, key: key)
          return
        }
        complete(key: key, state: .ready(artifact))
      } catch is CancellationError {
        complete(key: key, state: nil)
      } catch {
        complete(key: key, state: Self.failureState(error))
      }
    }
  }

  private func invalidateCorrupt(
    artifact: RadrootsVerifiedMediaArtifact,
    context: RadrootsLocalNetwork,
    key: String
  ) async {
    _ = try? await runtimeClient.invalidateMediaArtifact(
      context: context,
      artifactID: artifact.artifactID
    )
    complete(key: key, state: .corrupt)
  }

  private func complete(key: String, state: RadrootsMediaPresentationState?) {
    tasks[key] = nil
    states[key] = state
  }

  private static func key(
    media: RadrootsMediaReference,
    context: RadrootsLocalNetwork
  ) -> String {
    "\(context.id)\u{0}\(context.generation)\u{0}\(media.referenceFingerprint)"
  }

  private static func failureState(_ error: Error) -> RadrootsMediaPresentationState {
    let failure: RadrootsRuntimeFailure? =
      if case let RadrootsRuntimeClientError.support(value) = error {
        value
      } else {
        error as? RadrootsRuntimeFailure
      }
    guard let failure else { return .failed }
    if failure.code.contains("corrupt") || failure.code.contains("verification") {
      return .corrupt
    }
    if failure.retryable
      || failure.category.localizedCaseInsensitiveContains("network")
      || failure.category.localizedCaseInsensitiveContains("relay")
    {
      return .offline
    }
    return .failed
  }
}
