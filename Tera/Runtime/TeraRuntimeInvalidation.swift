import Foundation

enum TeraRuntimeChangeKind: Sendable, Equatable, Hashable {
  case initial
  case identity
  case settings
  case profile
  case today
  case drafts
  case relay
  case media
  case lifecycle
}

enum TeraRuntimeChangeDelivery: Sendable, Equatable {
  case change
  case resnapshotRequired
}

struct TeraRuntimeChangeScope: Sendable, Equatable, Hashable {
  let publicKey: String
  let sourceGeneration: String
  let context: TeraLocalNetwork?
}

/// A hint to query state, never a durable mutation receipt or host session ID.
/// Revisions are comparable within one epoch, domain and exact scope only.
struct TeraRuntimeChange: Sendable, Equatable {
  let schemaVersion: UInt16
  let scope: TeraRuntimeChangeScope
  let epoch: String
  let revision: TeraProjectionRevision
  let delivery: TeraRuntimeChangeDelivery
  let kind: TeraRuntimeChangeKind
  let entityID: String?

  func matches(_ configuration: TeraRuntimeLaunchConfiguration?) -> Bool {
    guard let configuration else { return false }
    return schemaVersion == 3
      && Self.isHex(epoch, count: 32)
      && Self.isHex(scope.publicKey, count: 64)
      && Self.isHex(scope.sourceGeneration, count: 64)
      && scope.publicKey == configuration.publicKeyHex
      && scope.sourceGeneration == configuration.sourceGenerationHex
      && (scope.context == nil || scope.context?.schemaVersion == 1)
  }

  func matches(context: TeraLocalNetwork?) -> Bool {
    scope.context == nil || scope.context == context
  }

  func requiringResnapshot() -> Self {
    Self(
      schemaVersion: schemaVersion,
      scope: TeraRuntimeChangeScope(publicKey: scope.publicKey, sourceGeneration: scope.sourceGeneration, context: nil),
      epoch: epoch, revision: TeraProjectionRevision(rawValue: 0),
      delivery: .resnapshotRequired, kind: .initial, entityID: nil
    )
  }

  /// Loss leaves an account-wide gap queued or already delivered, even if this
  /// is the producer's final event. The existing buffer capacity stays fixed.
  func yield(to continuation: AsyncStream<Self>.Continuation) {
    if case .dropped = continuation.yield(self) {
      continuation.yield(requiringResnapshot())
    }
  }

  private static func isHex(_ value: String, count: Int) -> Bool {
    value.utf8.count == count && value.utf8.contains(where: { $0 != 48 })
      && value.utf8.allSatisfy { (48 ... 57).contains($0) || (97 ... 102).contains($0) }
  }
}

/// One subscription retains at most one watermark for each of the nine domains.
/// Domain counters cover all contexts; context filtering happens at each store.
struct TeraInvalidationAdmission {
  private var epoch: String?
  private var revisions: [TeraRuntimeChangeKind: TeraProjectionRevision] = [:]

  mutating func accept(_ change: TeraRuntimeChange) -> Bool {
    guard epoch == nil || epoch == change.epoch else { return false }
    epoch = change.epoch
    if change.delivery == .resnapshotRequired {
      return true
    }
    if let previous = revisions[change.kind], let next = change.revision.rawValue {
      guard let value = previous.rawValue, next > value else { return false }
    }
    revisions[change.kind] = change.revision
    return true
  }
}

struct TeraRuntimeSubscription {
  let generation: TeraSessionGeneration
  let continuation: AsyncStream<TeraRuntimeChange>.Continuation
  var token: (any TeraRuntimeSubscriptionToken)?
  var admission = TeraInvalidationAdmission()
}
