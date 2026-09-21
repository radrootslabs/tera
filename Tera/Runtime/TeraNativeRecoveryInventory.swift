import Foundation
import RadrootsKit

struct TeraNativeRecoveryInspection: Sendable {
  var selectedKey: String?
  var completedNeedsRepair: @Sendable (String) async throws -> Bool = { _ in false }
}

/// One explicit bounded pass over authoritative native receipts. The owner
/// persists a redacted ordering key after each visited position. Interrupted
/// work is replayed idempotently; a new sweep revisits earlier quarantined IDs.
enum TeraNativeRecoveryInventory {
  static let passLimit = 64

  static func run(
    transfer: any RadrootsBackgroundTransfer, cursor: String?,
    inspection: TeraNativeRecoveryInspection = .init(),
    checkpoint: @Sendable (String?) async throws -> Void = { _ in },
    complete: @Sendable (RadrootsBackgroundTransferSnapshot, TeraNativeUploadRecoveryOwner) async throws -> Void = { _, _ in throw TeraComposerAcknowledgment.unconfirmed },
    report: @Sendable (RadrootsBackgroundTransferSnapshot, TeraNativeRecoveryReason) async -> TeraNativeRecoveryIssue? = { snapshot, reason in
      reason == .resolved ? nil : .init(key: TeraNativeRecoveryIssue.key(snapshot.identifier.rawValue), reason: reason, status: nil)
    },
    lookup: @Sendable (String) async throws -> TeraNativeUploadRecoveryOwner?
  ) async throws -> (progress: TeraNativeRecoveryProgress, cursor: String?) {
    let snapshots = try await transfer.snapshots()
      .filter { $0.state == .awaitingVerification || $0.state == .completed }
      .map { (key: TeraNativeRecoveryIssue.key($0.identifier.rawValue), snapshot: $0) }
      .sorted { $0.key < $1.key }
    let pending = snapshots.filter {
      if let selected = inspection.selectedKey {
        return $0.key == selected
      }
      return cursor == nil || $0.key > (cursor ?? "")
    }
    guard inspection.selectedKey == nil || pending.count == 1 else { throw TeraComposerAcknowledgment.unconfirmed }
    var attention = false
    var issues: [TeraNativeRecoveryIssue] = []
    var pause: TeraNativeRecoveryPause?
    var visited = 0
    var last = cursor
    recovery: for position in pending.prefix(passLimit) {
      let snapshot = position.snapshot
      try Task.checkCancellation()
      switch try await recover(snapshot, inspection: inspection, complete: complete, lookup: lookup) {
      case .complete:
        if let resolved = await report(snapshot, .resolved) {
          issues.append(resolved)
          attention = attention || resolved.reason != .resolved
        }
      case .ignored: break
      case let .issue(reason):
        attention = true
        if let issue = await report(snapshot, reason) {
          issues.append(issue)
        }
      case let .pause(value):
        pause = value
        break recovery // Revisit this same item after unlock/storage recovery.
      }
      try await checkpoint(position.key)
      try Task.checkCancellation()
      visited += 1
      last = position.key
    }
    let remaining = pending.count - visited
    if remaining == 0 {
      try await checkpoint(nil)
    }
    return (.init(visited: visited, remaining: remaining, needsAttention: attention, issues: issues, pause: pause), remaining > 0 ? last : nil)
  }

  private enum Outcome { case complete, ignored, issue(TeraNativeRecoveryReason), pause(TeraNativeRecoveryPause) }

  private static func recover(_ snapshot: RadrootsBackgroundTransferSnapshot,
                              inspection: TeraNativeRecoveryInspection,
                              complete: @Sendable (RadrootsBackgroundTransferSnapshot, TeraNativeUploadRecoveryOwner) async throws -> Void,
                              lookup: @Sendable (String) async throws -> TeraNativeUploadRecoveryOwner?) async throws -> Outcome
  {
    do {
      if snapshot.state == .completed, inspection.selectedKey == nil,
         try await !inspection.completedNeedsRepair(TeraNativeRecoveryIssue.key(snapshot.identifier.rawValue)) {
           return .ignored
         }
      guard let identity = TeraBackgroundUploadRequest.transferIdentity(snapshot.identifier) else { throw TeraNativeRecoveryFault.associationMismatch }
      guard let owner = try await lookup(identity.draftID) else { throw TeraNativeRecoveryFault.missingParent }
      guard owner.id == identity.draftID else { throw TeraNativeRecoveryFault.associationMismatch }
      try Task.checkCancellation()
      try await complete(snapshot, owner)
      return .complete
    } catch {
      try Task.checkCancellation()
      if let pause = TeraNativeRecoveryClassification.pause(error) {
        return .pause(pause)
      }
      return .issue(TeraNativeRecoveryClassification.reason(error))
    }
  }
}
