import Foundation
import RadrootsKit

/// Reads only an already-owned native response. Never enqueues or retries work.
enum TeraStoppedUploadRecovery {
  static func receipt(_ submission: TeraSubmissionStatus, media: TeraPreparedMedia,
                      transfer: any RadrootsBackgroundTransfer) async throws -> TeraAddBackgroundUploadReceipt?
  {
    guard submission.delivery.isStopped,
          let item = submission.media.first(where: { $0.opaqueReference == media.opaqueReference }),
          item.progress.stage == .uploading else { throw TeraComposerAcknowledgment.unconfirmed }
    let snapshots = try await transfer.snapshots()
    try Task.checkCancellation()
    let candidates = try snapshots.filter { snapshot in
      guard snapshot.identifier.rawValue.hasPrefix("radroots.add.\(submission.intentID).") else { return false }
      guard let identity = TeraBackgroundUploadRequest.transferIdentity(snapshot.identifier),
            identity.draftID == submission.intentID, identity.revision <= submission.revision
      else { throw TeraComposerAcknowledgment.unconfirmed }
      guard snapshot.request.expectedSourceSHA256 == media.sha256 else { return false }
      guard try TeraBackgroundUploadRequest.persistedRequestMatchesMedia(
        snapshot.request, media: media, uploadURL: item.progress.uploadURL
      ) else { throw TeraComposerAcknowledgment.unconfirmed }
      return true
    }
    guard candidates.count <= 1 else { throw TeraComposerAcknowledgment.unconfirmed }
    guard let snapshot = candidates.first,
          [.awaitingVerification, .completed].contains(snapshot.state) else { return nil }
    guard let response = snapshot.response, let status = UInt16(exactly: response.statusCode),
          let body = response.body else { throw TeraComposerAcknowledgment.unconfirmed }
    return TeraAddBackgroundUploadReceipt(identifier: snapshot.identifier.rawValue, draftID: submission.intentID,
                                          expectedRevision: submission.revision, statusCode: status,
                                          mediaType: response.mediaType, contentEncoding: response.contentEncoding, body: body)
  }
}
