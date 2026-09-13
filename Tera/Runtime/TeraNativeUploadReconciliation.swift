import Foundation
import RadrootsKit

enum TeraNativeUploadReconciliation {
  static func reconcile(_ owners: [TeraNativeUploadRecoveryOwner], transfer: any RadrootsBackgroundTransfer) async throws {
    var draftsByID: [String: TeraNativeUploadRecoveryOwner] = [:]
    for draft in owners {
      guard draftsByID.updateValue(draft, forKey: draft.id) == nil else {
        throw failure(
          code: "ios.add.background_draft_ambiguous",
          message: "The persisted draft inventory is ambiguous."
        )
      }
    }
    for snapshot in try await transfer.snapshots()
    where snapshot.state == .awaitingVerification {
      try Task.checkCancellation()
      guard let identity = TeraBackgroundUploadRequest.transferIdentity(snapshot.identifier),
        let draft = draftsByID[identity.draftID]
      else { continue }
      guard draft.revision > identity.revision,
        let media = draft.media.first(where: {
          $0.sha256 == snapshot.request.expectedSourceSHA256
            && ($0.remoteURL == snapshot.request.remoteURL.absoluteString
              || $0.remoteURL.flatMap { draft.uploadURLs[$0] } == snapshot.request.remoteURL.absoluteString)
        }),
        let canonicalURL = media.remoteURL, draft.verifiedURLs.contains(canonicalURL),
        try TeraBackgroundUploadRequest.persistedRequestMatchesMedia(
          snapshot.request, media: media, uploadURL: draft.uploadURLs[canonicalURL]
        )
      else {
        throw failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload does not match its verified draft."
        )
      }
      try await transfer.settle(snapshot.identifier, verification: .accepted)
    }
  }

  private static func failure(code: String, message: String) -> TeraRuntimeFailure {
    .local(operation: "add.media.background", code: code, safeMessage: message)
  }
}
