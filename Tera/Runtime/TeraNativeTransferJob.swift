/// Immutable native transport input, independent of legacy/scoped storage models.
struct TeraNativeTransferJob: Sendable, Equatable, CustomStringConvertible, CustomDebugStringConvertible {
  let ownerID: String
  let expectedRevision: UInt64
  let operationID: String
  let remoteURL: String
  let uploadURL: String
  let authorizationHeader: String
  let expectedSHA256: String
  let mediaType: String
  let byteSize: UInt64
  var description: String {
    "TeraNativeTransferJob"
  }

  var debugDescription: String {
    description
  }
}

extension TeraNativeUploadJob {
  var transfer: TeraNativeTransferJob {
    TeraNativeTransferJob(ownerID: draft.id, expectedRevision: draft.revision, operationID: operationID,
                          remoteURL: remoteURL, uploadURL: uploadURL, authorizationHeader: authorizationHeader,
                          expectedSHA256: expectedSHA256, mediaType: mediaType, byteSize: byteSize)
  }
}

/// Presentation evidence used only to reconcile an already persisted OS transfer.
struct TeraNativeUploadRecoveryOwner: Sendable {
  let id: String
  let revision: UInt64
  let media: [TeraPreparedMedia]
  let verifiedURLs: Set<String>
  let uploadURLs: [String: String]
}

struct TeraNativeUploadJob: Sendable, Equatable {
  let operationID: String
  let draft: TeraDraftStatus
  let remoteURL: String
  let uploadURL: String
  let authorizationHeader: String
  let expectedSHA256: String
  let mediaType: String
  let byteSize: UInt64
}
