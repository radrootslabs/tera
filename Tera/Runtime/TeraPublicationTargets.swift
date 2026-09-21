import Foundation
import TeraKitBindings

enum TeraPublicationTargetPolicy: Sendable, Equatable {
  case any, all, quorum(UInt16), required([String])
}

struct TeraPublicationTarget: Sendable, Equatable, Identifiable {
  let id: String
  let endpoint: String
  let attempted: Bool
  let accepted: Bool
  let delivered: Bool
  let rejected: Bool
  let uncertain: Bool
  let readBackObservedAtUnixMilliseconds: UInt64?

  var summary: String {
    if delivered {
      return "Delivery confirmed by this relay."
    }
    if accepted {
      return "Accepted by this relay."
    }
    if uncertain {
      return "Delivery outcome is uncertain."
    }
    if rejected {
      return "A refusal from this relay is recorded."
    }
    return attempted ? "Attempt recorded; acceptance is unconfirmed." : "No attempt is recorded for this relay."
  }
}

struct TeraPublicationTargets: Sendable, Equatable {
  let requiresDelivery: Bool
  let policy: TeraPublicationTargetPolicy
  let targets: [TeraPublicationTarget]
  let readBackAvailable: Bool
  let readBackComplete: Bool

  var policySummary: String {
    let level = requiresDelivery ? "delivery confirmation" : "acceptance"
    switch policy {
    case .any: return "Saved policy: \(level) from any one saved relay."
    case .all: return "Saved policy: \(level) from every saved relay."
    case let .quorum(count): return "Saved policy: \(level) from \(count) saved relays."
    case let .required(ids): return "Saved policy: \(level) from \(ids.count) specifically required relays."
    }
  }

  func isRequired(_ id: String) -> Bool {
    if case let .required(ids) = policy {
      return ids.contains(id)
    }
    return policy == .all
  }

  func follows(_ previous: Self) -> Bool {
    guard policy == previous.policy, requiresDelivery == previous.requiresDelivery,
          targets.map(\.id) == previous.targets.map(\.id) else { return false }
    return zip(targets, previous.targets).allSatisfy { current, old in
      current.endpoint == old.endpoint && (!old.attempted || current.attempted)
        && (!old.accepted || current.accepted) && (!old.delivered || current.delivered)
        && (!old.rejected || current.rejected)
    }
  }

  static func decode(_ value: FfiPublicationTargetDetails) throws -> Self {
    guard (1 ... 64).contains(value.targets.count), value.readBackAvailable || !value.readBackComplete
    else { throw TeraGeneratedSubmission.mismatch() }
    let targets = try value.targets.map { wire in
      guard fingerprint(wire.fingerprint), !wire.endpoint.isEmpty, wire.endpoint.utf8.count <= 2048,
            !wire.endpoint.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains),
            !wire.accepted || wire.attempted, !wire.delivered || wire.accepted,
            !wire.rejected || wire.attempted,
            wire.readBackObservedAtUnixMs.map({ $0 > 0 && $0 <= UInt64(Int64.max) }) != false,
            value.readBackAvailable || wire.readBackObservedAtUnixMs == nil
      else { throw TeraGeneratedSubmission.mismatch() }
      return TeraPublicationTarget(id: wire.fingerprint, endpoint: wire.endpoint, attempted: wire.attempted,
                                   accepted: wire.accepted, delivered: wire.delivered, rejected: wire.rejected,
                                   uncertain: wire.uncertain, readBackObservedAtUnixMilliseconds: wire.readBackObservedAtUnixMs)
    }
    let ids = Set(targets.map(\.id))
    guard ids.count == targets.count else { throw TeraGeneratedSubmission.mismatch() }
    let policy: TeraPublicationTargetPolicy
    switch value.policy {
    case .any: policy = .any
    case .all: policy = .all
    case let .quorum(threshold):
      guard threshold > 0, threshold <= targets.count else { throw TeraGeneratedSubmission.mismatch() }
      policy = .quorum(threshold)
    case let .required(required):
      guard !required.isEmpty, required.count <= targets.count, Set(required).count == required.count,
            required.allSatisfy(ids.contains) else { throw TeraGeneratedSubmission.mismatch() }
      policy = .required(required)
    }
    return Self(requiresDelivery: value.requiresDelivery, policy: policy, targets: targets,
                readBackAvailable: value.readBackAvailable, readBackComplete: value.readBackComplete)
  }

  private static func fingerprint(_ value: String) -> Bool {
    value.utf8.count == 64 && value.utf8.allSatisfy { (48 ... 57).contains($0) || (97 ... 102).contains($0) }
  }
}
