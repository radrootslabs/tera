import Foundation
import RadrootsKit

struct TeraRecoveryUploadReceipt: Sendable {
  let parent: String
  let revision: UInt64
  let attempt: String
  let uploadURL: String
  let media: TeraPreparedMedia
  let response: TeraAddBackgroundUploadReceipt

  init(snapshot: RadrootsBackgroundTransferSnapshot, owner: TeraNativeUploadRecoveryOwner) throws {
    guard snapshot.state == .awaitingVerification,
          let identity = TeraBackgroundUploadRequest.transferIdentity(snapshot.identifier),
          identity.draftID == owner.id, identity.revision <= owner.revision,
          let hash = snapshot.request.expectedSourceSHA256,
          let media = owner.media.first(where: { $0.sha256 == hash }),
          owner.media.filter({ $0.sha256 == hash }).count == 1,
          let url = media.remoteURL, let uploadURL = owner.uploadURLs[url],
          snapshot.request.remoteURL.absoluteString == uploadURL,
          try TeraBackgroundUploadRequest.persistedRequestMatchesMedia(snapshot.request, media: media, uploadURL: uploadURL),
          let response = snapshot.response, let status = UInt16(exactly: response.statusCode),
          let body = response.body,
          body.count <= 16384, (response.mediaType?.utf8.count ?? 0) <= 8192,
          (response.contentEncoding?.utf8.count ?? 0) <= 8192,
          body.count + (response.mediaType?.utf8.count ?? 0) + (response.contentEncoding?.utf8.count ?? 0) <= 16384
    else { throw TeraComposerAcknowledgment.unconfirmed }
    parent = identity.draftID
    revision = identity.revision
    attempt = identity.attempt
    self.uploadURL = uploadURL
    self.media = media
    self.response = .init(identifier: snapshot.identifier.rawValue, draftID: parent, expectedRevision: revision,
                          statusCode: status, mediaType: response.mediaType, contentEncoding: response.contentEncoding, body: body)
  }
}

struct TeraRecoveryCompletionReceipt: Sendable, Equatable {
  let parent: String
  let attempt: String
  let canonicalURL: String
  let sha256: String
  let mediaType: String
  let byteSize: UInt64
  let verifiedAtUnixMS: UInt64

  func confirm(_ input: TeraRecoveryUploadReceipt) throws {
    guard parent == input.parent, attempt == input.attempt, canonicalURL == input.media.remoteURL,
          sha256 == input.media.sha256, mediaType == input.media.mediaType, byteSize == input.media.byteSize,
          verifiedAtUnixMS > 0, verifiedAtUnixMS <= UInt64(Int64.max) else { throw TeraComposerAcknowledgment.unconfirmed }
  }
}

extension TeraRuntimeClient {
  func recoverNativeUpload(_ receipt: TeraRecoveryUploadReceipt, media: TeraPreparedMediaHandle) async throws -> TeraRecoveryCompletionReceipt {
    try await addOperation("runtime.recovery.complete") { try await $0.recoverNativeUpload(receipt, media: media) }
  }
}

extension TeraRuntimeBackend {
  func recoverNativeUpload(_: TeraRecoveryUploadReceipt, media _: TeraPreparedMediaHandle) async throws -> TeraRecoveryCompletionReceipt {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}
