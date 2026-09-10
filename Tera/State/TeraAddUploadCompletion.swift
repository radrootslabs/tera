import Foundation

enum TeraAddUploadCompletion {
  static func complete(
    _ receipt: TeraAddBackgroundUploadReceipt,
    handle: TeraPreparedMediaHandle,
    media: any TeraAddMediaHandling,
    runtimeClient: TeraRuntimeClient
  ) async throws -> TeraDraftStatus {
    do {
      return try await runtimeClient.completeAddMediaBackground(
        input: TeraNativeUploadCompletion(
          draftID: receipt.draftID,
          expectedRevision: receipt.expectedRevision,
          media: handle,
          statusCode: receipt.statusCode,
          responseMediaType: receipt.mediaType,
          responseContentEncoding: receipt.contentEncoding,
          responseBody: receipt.body
        )
      )
    } catch is CancellationError {
      // Rust completion may already be durable. Leave the receipt awaiting
      // verification so relaunch can reconcile the unknown outcome.
      throw CancellationError()
    } catch {
      if TeraAddPresentation.failure(for: error)?.code == "ios.runtime.cancelled" {
        // The bounded runtime client cannot prove whether a cancelled FFI
        // completion became durable. Preserve the receipt for reconciliation.
        throw CancellationError()
      }
      try? await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: false)
      throw error
    }
  }
}
