import Foundation
import RadrootsKit

/// Rust has already confirmed the exact attempt. Native settlement cannot
/// create or advance that completion, including after a lost native reply.
enum TeraNativeUploadReconciliation {
  static func settle(_ expected: RadrootsBackgroundTransferSnapshot, input: TeraRecoveryUploadReceipt,
                     receipt: TeraRecoveryCompletionReceipt, transfer: any RadrootsBackgroundTransfer) async throws
  {
    try receipt.confirm(input)
    guard input.response.identifier == expected.identifier.rawValue,
          input.uploadURL == expected.request.remoteURL.absoluteString,
          input.media.sha256 == expected.request.expectedSourceSHA256 else { throw TeraComposerAcknowledgment.unconfirmed }
    let current = try await matchingSnapshot(expected, transfer: transfer)
    try Task.checkCancellation()
    if current.state == .completed {
      return
    }
    guard current.state == .awaitingVerification else { throw TeraComposerAcknowledgment.unconfirmed }
    do {
      try await transfer.settle(expected.identifier, verification: .accepted)
    } catch {
      try Task.checkCancellation()
      // A storage or transport reply can be lost after its state is committed.
      // Only matching durable native success makes this success-equivalent.
      let recovered = try await matchingSnapshot(expected, transfer: transfer)
      guard recovered.state == .completed else { throw error }
      return
    }
    let confirmed = try await matchingSnapshot(expected, transfer: transfer)
    guard confirmed.state == .completed else { throw TeraComposerAcknowledgment.unconfirmed }
  }

  private static func matchingSnapshot(_ expected: RadrootsBackgroundTransferSnapshot,
                                       transfer: any RadrootsBackgroundTransfer) async throws -> RadrootsBackgroundTransferSnapshot
  {
    guard let current = try await transfer.snapshot(for: expected.identifier),
          current.identifier == expected.identifier, current.request == expected.request,
          current.response == expected.response, current.executionID == expected.executionID,
          current.uploadLease == expected.uploadLease else { throw TeraComposerAcknowledgment.unconfirmed }
    return current
  }
}
