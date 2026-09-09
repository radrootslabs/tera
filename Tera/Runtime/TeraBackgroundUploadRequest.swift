import Foundation
import RadrootsKit

/// Exact request translation and matching; no transfer or task ownership.
enum TeraBackgroundUploadRequest {
  static let operationInProgress = TeraRuntimeFailure(
    schemaVersion: 1, code: "operation_in_progress", category: "operation",
    retryable: true, recoveryActions: ["retry_operation_with_same_idempotency_key"],
    operationID: "add.media.background", capabilityID: nil,
    safeMessage: "A photo upload is already in progress for this draft."
  )

  static func prepare(
    job: TeraNativeUploadJob,
    media: TeraPreparedMedia,
    preparer: RadrootsAppleMediaPreparer
  ) async throws -> RadrootsBackgroundTransferRequest {
    guard job.expectedSHA256 == media.sha256,
      job.mediaType == media.mediaType,
      job.byteSize == media.byteSize,
      let byteSize = Int(exactly: media.byteSize),
      let remoteURL = URL(string: job.remoteURL)
    else {
      throw TeraRuntimeFailure.local(
        operation: "add.media.background",
        code: "ios.add.background_upload_mismatch",
        safeMessage: "The prepared photo no longer matches the authorized upload."
      )
    }
    let identifier = try TeraBackgroundUploadRequest.transferIdentifier(job: job)
    let prepared = try RadrootsApplePreparedImage(
      file: RadrootsStagedBlobReference(
        blobID: media.sha256,
        sizeBytes: byteSize,
        mediaType: media.mediaType,
        filenameHint: "\(media.sha256).png"
      ),
      sha256: media.sha256,
      width: media.width,
      height: media.height
    )
    return try await preparer.blossomUploadRequest(
      preparedImage: prepared,
      remoteURL: remoteURL,
      authorization: job.authorizationHeader,
      networkPolicy: remoteURL.scheme?.lowercased() == "https"
        ? .publicHTTPS : .simulatorLoopbackHTTP,
      identifier: identifier
    )
  }

  static func transferIdentifier(
    job: TeraNativeUploadJob
  ) throws -> RadrootsBackgroundTransferIdentifier {
    try RadrootsBackgroundTransferIdentifier(
      "radroots.add.\(job.draft.id).\(job.draft.revision).\(job.operationID)"
    )
  }

  static func persistedRequestMatches(
    _ persisted: RadrootsBackgroundTransferRequest,
    request: RadrootsBackgroundTransferRequest
  ) -> Bool {
    persisted.headers.isEmpty && persisted.metadata.isEmpty
      && persisted.remoteURL == request.remoteURL
      && persisted.method == request.method
      && persisted.operation == request.operation
      && persisted.networkPolicy == request.networkPolicy
      && persisted.responsePolicy == request.responsePolicy
      && persisted.expectedSourceSHA256 == request.expectedSourceSHA256
      && persisted.maximumTransferBytes == request.maximumTransferBytes
  }

  static func persistedRequestMatchesMedia(
    _ persisted: RadrootsBackgroundTransferRequest,
    media: TeraPreparedMedia
  ) throws -> Bool {
    guard let remoteURL = media.remoteURL.flatMap(URL.init(string:)),
      let byteSize = Int(exactly: media.byteSize)
    else { return false }
    let blob = try RadrootsStagedBlobReference(
      blobID: media.sha256,
      sizeBytes: byteSize,
      mediaType: media.mediaType,
      filenameHint: "\(media.sha256).png"
    )
    let responsePolicy = try RadrootsBackgroundTransferResponsePolicy.boundedJSON()
    return persisted.headers.isEmpty && persisted.metadata.isEmpty
      && persisted.remoteURL == remoteURL
      && persisted.method == .put
      && persisted.operation == .upload(source: .stagedBlob(blob))
      && persisted.networkPolicy
        == (remoteURL.scheme?.lowercased() == "https" ? .publicHTTPS : .simulatorLoopbackHTTP)
      && persisted.responsePolicy == responsePolicy
      && persisted.expectedSourceSHA256 == media.sha256
      && persisted.maximumTransferBytes
        == RadrootsBackgroundTransferRequest.defaultMaximumTransferBytes
  }

  static func replacingIdentifier(
    in request: RadrootsBackgroundTransferRequest,
    with identifier: RadrootsBackgroundTransferIdentifier
  ) throws -> RadrootsBackgroundTransferRequest {
    try RadrootsBackgroundTransferRequest(
      identifier: identifier,
      remoteURL: request.remoteURL,
      method: request.method,
      operation: request.operation,
      headers: request.headers,
      metadata: request.metadata,
      networkPolicy: request.networkPolicy,
      responsePolicy: request.responsePolicy,
      expectedSourceSHA256: request.expectedSourceSHA256,
      maximumTransferBytes: request.maximumTransferBytes
    )
  }

  static func transferIdentity(
    _ identifier: RadrootsBackgroundTransferIdentifier
  ) -> (draftID: String, revision: UInt64)? {
    let components = identifier.rawValue.split(separator: ".", omittingEmptySubsequences: false)
    guard components.count == 5,
      components[0] == "radroots",
      components[1] == "add",
      components[2].range(of: "^[0-9a-f]{32}$", options: .regularExpression) != nil,
      let revision = UInt64(components[3]),
      String(revision) == components[3],
      components[4].range(of: "^[0-9a-f]{32}$", options: .regularExpression) != nil
    else { return nil }
    return (String(components[2]), revision)
  }
}
