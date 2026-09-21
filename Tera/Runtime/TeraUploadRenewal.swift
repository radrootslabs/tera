import Foundation
import RadrootsKit

/// Explicit foreground user action only. Rust owns budget, expiry and signing.
struct TeraUploadRenewal: Sendable {
  let transfer: any RadrootsBackgroundTransfer
  let preparer: RadrootsAppleMediaPreparer
  struct Prior: Sendable {
    let identifier: RadrootsBackgroundTransferIdentifier
    let attempt: String
    let revision: UInt64
  }

  func run(_ submission: TeraSubmissionStatus, media: TeraPreparedMedia, handle: TeraPreparedMediaHandle,
           backend: any TeraRuntimeBackend) async throws -> TeraSubmissionStatus
  {
    try Task.checkCancellation()
    guard !submission.delivery.isStopped,
          let item = submission.media.first(where: { $0.opaqueReference == media.opaqueReference }),
          !item.authorizations.isEmpty, item.authorizations.count <= 5,
          let uploadURL = item.progress.uploadURL else { throw TeraNativeUploadExecution.unknown }
    let inventory = try await transfer.snapshots()
    let lineage = try Self.identities(item.authorizations, parent: submission.intentID, inventory: inventory)
    let known = Set(lineage.map(\.identifier))
    for snapshot in inventory where snapshot.identifier.rawValue.hasPrefix("radroots.add.\(submission.intentID).")
      && snapshot.request.expectedSourceSHA256 == media.sha256
    {
      guard known.contains(snapshot.identifier),
            try TeraBackgroundUploadRequest.persistedRequestMatchesMedia(snapshot.request, media: media, uploadURL: uploadURL)
      else { throw TeraNativeUploadExecution.unknown }
    }
    guard let latest = lineage.last else { throw TeraNativeUploadExecution.unknown }
    return try await Self.holding(lineage, transfer: transfer) { snapshots in
      try Task.checkCancellation()
      for snapshot in snapshots {
        guard try TeraBackgroundUploadRequest.persistedRequestMatchesMedia(snapshot.request, media: media, uploadURL: uploadURL)
        else { throw TeraNativeUploadExecution.unknown }
      }
      let retry = TeraSubmissionUploadRenewal(priorRevision: latest.revision, priorAttempt: latest.attempt,
                                              nativeFailed: snapshots.contains { $0.identifier == latest.identifier && [.failed, .expired].contains($0.state) })
      let input = TeraSubmissionMediaRequest(request: submission.request, expectedRevision: submission.revision, media: handle)
      let policy: RadrootsBackgroundTransferNetworkPolicy = uploadURL.hasPrefix("https:") ? .publicHTTPS : .simulatorLoopbackHTTP
      if !RadrootsAppleBackgroundTransferAdapters.supportsNewEnqueue(for: policy) {
        return try await backend.renewSubmissionMedia(input: input, renewal: retry)
      }
      let job = try await backend.renewSubmissionUpload(input: input, renewal: retry)
      try Task.checkCancellation()
      let current = try await backend.submissionStatus(request: submission.request)
      guard !current.delivery.isStopped, current.revision == job.submission.revision else { throw TeraNativeUploadExecution.unknown }
      let request = try await TeraBackgroundUploadRequest.prepare(job: job.transfer, media: media, preparer: preparer)
      guard !known.contains(request.identifier) else { throw TeraNativeUploadExecution.unknown }
      let active = try await TeraNativeUploadExecution.start(transfer: transfer, request: request, retrying: false)
      let receipt = try await TeraBackgroundUploadWaiter.receipt(transfer: transfer, for: active.identifier,
                                                                 draftID: submission.intentID, expectedRevision: job.submission.revision, request: request, baseline: active)
      let status = try await backend.completeSubmissionUpload(input: .init(request: submission.request,
                                                                           expectedRevision: receipt.expectedRevision, media: handle), response: receipt)
      try await transfer.settle(active.identifier, verification: .accepted)
      return status
    }
  }

  static func identities(_ attempts: [TeraUploadAttemptIdentity], parent: String,
                         inventory: [RadrootsBackgroundTransferSnapshot]) throws -> [Prior]
  {
    guard !attempts.isEmpty, attempts.count <= 5 else { throw TeraNativeUploadExecution.unknown }
    return try attempts.map { attempt in
      let matches = inventory.compactMap { snapshot -> Prior? in
        guard let identity = TeraBackgroundUploadRequest.transferIdentity(snapshot.identifier),
              identity.draftID == parent, identity.attempt == attempt.operationID else { return nil }
        return Prior(identifier: snapshot.identifier, attempt: identity.attempt, revision: identity.revision)
      }
      guard matches.count <= 1 else { throw TeraNativeUploadExecution.unknown }
      guard let revision = attempt.revision ?? matches.first?.revision, revision > 0,
            matches.first.map({ $0.revision == revision }) ?? true else { throw TeraNativeUploadExecution.unknown }
      return try Prior(identifier: RadrootsBackgroundTransferIdentifier("radroots.add.\(parent).\(revision).\(attempt.operationID)"),
                       attempt: attempt.operationID, revision: revision)
    }
  }

  static func holding<Result: Sendable>(_ lineage: [Prior], transfer: any RadrootsBackgroundTransfer,
                                        snapshots: [RadrootsBackgroundTransferSnapshot] = [],
                                        operation: @escaping @Sendable ([RadrootsBackgroundTransferSnapshot]) async throws -> Result) async throws -> Result
  {
    guard let first = lineage.first else { return try await operation(snapshots) }
    return try await transfer.withInactiveExecution(for: first.identifier) { snapshot in
      try Task.checkCancellation()
      return try await holding(Array(lineage.dropFirst()), transfer: transfer,
                               snapshots: snapshots + (snapshot.map { [$0] } ?? []), operation: operation)
    }
  }
}
