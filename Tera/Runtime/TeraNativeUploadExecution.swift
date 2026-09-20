import Foundation
import RadrootsKit

/// A lost admission reply is not permission to start a second native task.
enum TeraNativeUploadExecution {
  static var unknown: TeraRuntimeFailure {
    .local(operation: "add.media.background", code: "ios.add.background_upload_unknown",
           safeMessage: "The photo upload outcome is not yet confirmed. Check recovery before trying again.")
  }

  static func start(transfer: any RadrootsBackgroundTransfer,
                    request: RadrootsBackgroundTransferRequest, retrying: Bool) async throws -> RadrootsBackgroundTransferSnapshot
  {
    try Task.checkCancellation()
    do {
      if retrying {
        _ = try await transfer.retry(request)
      } else {
        _ = try await transfer.enqueue(request)
      }
    } catch {
      try Task.checkCancellation()
      // A lost reply still requires the same exact query below; never retry.
    }
    // The producer queries OS tasks before returning reconciled snapshots.
    // Carry the observed execution into receipt waiting across this await.
    let snapshot = try await transfer.snapshot(for: request.identifier)
    try Task.checkCancellation()
    guard let snapshot, snapshot.identifier == request.identifier,
          TeraBackgroundUploadRequest.persistedRequestMatches(snapshot.request, request: request),
          [.queued, .running, .awaitingVerification, .completed].contains(snapshot.state)
    else { throw unknown }
    return snapshot
  }
}
