import Foundation
import RadrootsKit

/// One explicit bounded pass over authoritative native receipts. The owner
/// stores its cursor; cancellation replays the unfinished pass idempotently.
enum TeraNativeRecoveryInventory {
  static let passLimit = 64

  static func run(
    transfer: any RadrootsBackgroundTransfer, cursor: String?,
    complete: @Sendable (RadrootsBackgroundTransferSnapshot, TeraNativeUploadRecoveryOwner) async throws -> Void = { _, _ in throw TeraComposerAcknowledgment.unconfirmed },
    lookup: @Sendable (String) async throws -> TeraNativeUploadRecoveryOwner?
  ) async throws -> (progress: TeraNativeRecoveryProgress, cursor: String?) {
    let snapshots = try await transfer.snapshots()
      .filter { $0.state == .awaitingVerification }
      .sorted { $0.identifier.rawValue < $1.identifier.rawValue }
    let pending = snapshots.filter { cursor == nil || $0.identifier.rawValue > (cursor ?? "") }
    var attention = false
    var visited = 0
    var last = cursor
    for snapshot in pending.prefix(passLimit) {
      try Task.checkCancellation()
      do {
        guard let identity = TeraBackgroundUploadRequest.transferIdentity(snapshot.identifier),
              let owner = try await lookup(identity.draftID), owner.id == identity.draftID
        else { throw TeraComposerAcknowledgment.unconfirmed }
        try Task.checkCancellation()
        // A presentation status never substitutes for exact durable Rust proof.
        try await complete(snapshot, owner)
      } catch {
        try Task.checkCancellation()
        // Keep this receipt unchanged. One missing/ambiguous parent never
        // disables another item or the editable composer.
        attention = true
      }
      visited += 1
      last = snapshot.identifier.rawValue
    }
    let remaining = pending.count - visited
    // Explicit next invocation after end starts fresh to revisit earlier IDs.
    return (.init(visited: visited, remaining: remaining, needsAttention: attention), remaining > 0 ? last : nil)
  }
}
