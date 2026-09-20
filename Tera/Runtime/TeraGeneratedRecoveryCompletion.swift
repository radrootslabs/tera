import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  func recoverNativeUpload(_ receipt: TeraRecoveryUploadReceipt, media: TeraPreparedMediaHandle) async throws -> TeraRecoveryCompletionReceipt {
    do {
      guard media.media == receipt.media else { throw TeraComposerAcknowledgment.unconfirmed }
      let response = receipt.response
      let value = try await runtime.recoverNativeUpload(receipt: FfiRecoveryUploadReceipt(
        schemaVersion: 1, parent: receipt.parent, revision: receipt.revision, attempt: receipt.attempt,
        uploadUrl: receipt.uploadURL, sha256: receipt.media.sha256, mediaType: receipt.media.mediaType,
        byteSize: receipt.media.byteSize, response: FfiSubmissionUploadResponse(
          schemaVersion: 1, statusCode: response.statusCode, mediaType: response.mediaType,
          contentEncoding: response.contentEncoding, body: response.body
        )
      ), media: media.generatedValue)
      guard value.schemaVersion == 1 else { throw TeraComposerAcknowledgment.unconfirmed }
      let result = TeraRecoveryCompletionReceipt(parent: value.parent, attempt: value.attempt,
                                                 canonicalURL: value.canonicalUrl, sha256: value.sha256,
                                                 mediaType: value.mediaType, byteSize: value.byteSize,
                                                 verifiedAtUnixMS: value.verifiedAtUnixMs)
      try result.confirm(receipt)
      return result
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }
}
