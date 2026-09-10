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

  private struct CachedImage {
    let image: UIImage
    let cost: Int
  }

  @Published private var states: [Key: TeraMediaPresentationState] = [:]
  private let runtimeClient: TeraRuntimeClient
  private let limits: TeraMediaPresentationLimits
  private let work: TeraMediaWorkQueue
  private var owners: [Key: UUID] = [:]
  private var images: [Key: CachedImage] = [:]
  private var recent: [Key] = []
  private var revoked = Set<Key>()
  private var visibilityOverflow = false
  private var reauthorized = Set<Key>()
  private var configuration: TeraPresentationConfiguration?

  init(runtimeClient: TeraRuntimeClient, limits: TeraMediaPresentationLimits = .standard) {
    self.runtimeClient = runtimeClient
    self.limits = limits
    work = TeraMediaWorkQueue(limits: limits)
  }

  var cachedByteCount: Int {
    images.values.reduce(0) { $0 + $1.cost }
  }

  var stateCount: Int {
    states.count
  }

  func image(for media: TeraMediaReference, context: TeraLocalNetwork?) -> UIImage? {
    guard let context else { return nil }
    let key = key(media: media, context: context)
    guard allowed(key) else { return nil }
    touch(key)
    return images[key]?.image
  }

  func state(for media: TeraMediaReference, context: TeraLocalNetwork?) -> TeraMediaPresentationState {
    guard let context else { return .unavailable }
    let key = key(media: media, context: context)
    guard allowed(key) else { return .unavailable }
    if let state = states[key] {
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
    guard allowed(key), states[key] == nil, owners[key] == nil else { return }
    switch media.verification {
    case .pending: set(.pending, for: key)
    case .failed: set(.failed, for: key)
    case .verified:
      guard let artifactID = media.verifiedArtifactID else { set(.corrupt, for: key); return }
      start(key: key, context: context) { [runtimeClient] in
        try await runtimeClient.verifiedMediaArtifact(context: context, artifactID: artifactID)
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
    guard allowed(key) else { return }
    cancel(key)
    start(key: key, context: context) { [runtimeClient] in
      try await runtimeClient.retrieveMedia(context: context, reference: media)
    }
  }

  func reconcileVisibility(previous: [TeraMediaReference], current: [TeraMediaReference],
                           context: TeraLocalNetwork?)
  {
    guard let context else { return }
    let permitted = Set(current.prefix(limits.visibilityEntries).map { key(media: $0, context: context) })
    if current.count > limits.visibilityEntries {
      visibilityOverflow = true
    }
    reauthorized = permitted
    for key in permitted where revoked.remove(key) != nil {
      remove(key)
    }
    for reference in previous {
      let key = key(media: reference, context: context)
      guard !permitted.contains(key) else { continue }
      if !revoked.contains(key) {
        if revoked.count < limits.visibilityEntries {
          revoked.insert(key)
        } else {
          visibilityOverflow = true
        }
      }
      cancel(key)
    }
    // Under admission pressure only the bounded, explicitly current window may
    // load. Forgetting an old denial must never authorize a stale reference.
    if visibilityOverflow {
      for key in Array(owners.keys) where !allowed(key) {
        cancel(key)
      }
      for key in Array(states.keys) where !allowed(key) {
        remove(key)
      }
    }
  }

  func reset() {
    work.cancelAll()
    owners.removeAll()
    states.removeAll()
    images.removeAll()
    recent.removeAll()
    revoked.removeAll()
    reauthorized.removeAll()
    visibilityOverflow = false
  }

  func configure(snapshot: TeraRuntimeSnapshot) {
    let updated = TeraPresentationConfiguration(snapshot: snapshot)
    guard configuration != updated else { return }
    reset()
    configuration = updated
  }

  private func start(key: Key, context: TeraLocalNetwork,
                     operation: @escaping @Sendable () async throws -> TeraVerifiedMediaArtifact?)
  {
    let id = UUID()
    owners[key] = id
    set(.loading, for: key)
    let accepted = work.submit(id: id) { [weak self, runtimeClient, limits] in
      do {
        try Task.checkCancellation()
        guard let artifact = try await operation() else {
          self?.complete(key: key, id: id, state: .unavailable)
          return
        }
        guard self?.isCurrent(key, id) == true else { return }
        do {
          let image = try await TeraMediaThumbnail.prepare(artifact, limits: limits)
          guard let self, isCurrent(key, id) else { return }
          let cost = artifact.bytes.count + image.bytesPerRow * image.height
          guard cost <= limits.cacheBytes else {
            complete(key: key, id: id, state: .unavailable)
            return
          }
          images[key] = CachedImage(image: UIImage(cgImage: image), cost: cost)
          complete(key: key, id: id, state: .ready(artifact))
        } catch TeraMediaThumbnailFailure.corrupt {
          guard self?.isCurrent(key, id) == true else { return }
          _ = try? await runtimeClient.invalidateMediaArtifact(context: context, artifactID: artifact.artifactID)
          self?.complete(key: key, id: id, state: .corrupt)
        } catch TeraMediaThumbnailFailure.resourceLimit {
          self?.complete(key: key, id: id, state: .unavailable)
        }
      } catch is CancellationError {
        self?.complete(key: key, id: id, state: nil)
      } catch {
        self?.complete(key: key, id: id, state: Self.failureState(error))
      }
    }
    if !accepted {
      owners[key] = nil; set(.unavailable, for: key)
    }
  }

  private func isCurrent(_ key: Key, _ id: UUID) -> Bool {
    owners[key] == id && allowed(key) && !Task.isCancelled
  }

  private func complete(key: Key, id: UUID, state: TeraMediaPresentationState?) {
    guard owners[key] == id else { return }
    owners[key] = nil
    if Task.isCancelled || !allowed(key) {
      remove(key)
    } else {
      set(state, for: key)
    }
  }

  private func allowed(_ key: Key) -> Bool {
    visibilityOverflow ? reauthorized.contains(key) : !revoked.contains(key)
  }

  private func cancel(_ key: Key) {
    if let id = owners.removeValue(forKey: key) {
      work.cancel(id: id)
    }
    remove(key)
  }

  private func remove(_ key: Key) {
    states[key] = nil
    images[key] = nil
    recent.removeAll { $0 == key }
  }

  private func touch(_ key: Key) {
    guard states[key] != nil else { return }
    recent.removeAll { $0 == key }
    recent.append(key)
  }

  private func set(_ state: TeraMediaPresentationState?, for key: Key) {
    guard let state else { remove(key); return }
    states[key] = state
    touch(key)
    while states.count > limits.cacheEntries || cachedByteCount > limits.cacheBytes {
      guard let oldest = recent.first(where: { owners[$0] == nil }) else { break }
      remove(oldest)
    }
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
