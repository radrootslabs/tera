import Foundation

/// Bounded native scheduling over one immutable Rust-owned operation.
@MainActor
struct TeraSubmissionEffects {
  let client: TeraRuntimeClient
  let media: (any TeraAddMediaHandling)?
  let ensure: () throws -> Void
  let accept: (TeraSubmissionStatus) throws -> Void
  var mayStart: () -> Bool = { true }
  var stopControl: TeraSubmissionStopControl?

  func advance(_ initial: TeraSubmissionStatus) async throws {
    try ensure()
    if initial.delivery.isStopped {
      try await reconcileStopped(initial)
      return
    }
    guard mayStart() else { return }
    if [.complete, .terminal, .cancelled].contains(initial.state) {
      try await media?.reconcileBackgroundSubmissions([initial], client: client)
      try ensure()
      return
    }
    var current = initial
    if current.media.contains(where: { $0.progress.stage != .verified }) {
      current = try await upload(current)
    }
    try ensure()
    if mayStart(), !current.delivery.isStopped, ![.complete, .terminal, .cancelled].contains(current.state) {
      current = try await client.advanceSubmission(request: current.request, expectedRevision: current.revision)
      try accept(current)
    }
    try await media?.reconcileBackgroundSubmissions([current], client: client)
    try ensure()
  }

  private func upload(_ initial: TeraSubmissionStatus) async throws -> TeraSubmissionStatus {
    guard let media else {
      throw TeraRuntimeFailure.local(operation: "submission.media", code: "ios.add.media_unavailable",
                                     safeMessage: "Prepared photos are unavailable on this device.")
    }
    var current = initial
    let prepared = initial.preparedMedia
    let opened = try await TeraOpenedMedia.open(prepared, using: media)
    defer { opened.close() }
    try ensure()
    for item in initial.media where item.progress.stage != .verified {
      guard mayStart(), !current.delivery.isStopped else { return current }
      guard let source = prepared.first(where: { $0.opaqueReference == item.opaqueReference }),
            let handle = opened.handles.first(where: { $0.media.opaqueReference == item.opaqueReference })
      else {
        throw TeraComposerAcknowledgment.unconfirmed
      }
      if !item.authorizations.isEmpty {
        current = try await renew(current, source: source, using: media)
        continue
      }
      if try await media.prefersSharedForegroundUpload(ownerID: current.intentID) {
        let input = TeraSubmissionMediaRequest(request: current.request, expectedRevision: current.revision, media: handle)
        // View cancellation does not cancel the effect. The client retains
        // admission and Rust file ownership through any late deadline callback.
        current = try await Task { try await client.uploadSubmissionMedia(input: input) }.value
        try accept(current)
        try ensure()
        continue
      }
      let job = try await client.prepareSubmissionUpload(input: TeraSubmissionMediaRequest(
        request: current.request, expectedRevision: current.revision, media: handle
      ))
      try accept(job.submission)
      try ensure()
      guard mayStart(), !job.submission.delivery.isStopped else { return job.submission }
      let receipt = try await upload(job.transfer, source: source, using: media)
      try ensure()
      // Keep an uncertain OS receipt until Rust has durably verified it. A
      // storage/read/cancellation failure is never proof the upload was rejected.
      current = try await client.completeSubmissionUpload(input: TeraSubmissionMediaRequest(
        request: current.request, expectedRevision: job.submission.revision, media: handle
      ), response: receipt)
      try accept(current)
      try await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: true)
      _ = await TeraNativeRecoveryClassification.report(receipt.identifier, reason: .resolved, client: client)
      try ensure()
    }
    return current
  }

  private func upload(_ transfer: TeraNativeTransferJob, source: TeraPreparedMedia, using media: any TeraAddMediaHandling) async throws -> TeraAddBackgroundUploadReceipt {
    if let stopControl {
      return try await stopControl.upload(using: media, transfer: transfer, source: source)
    }
    return try await media.uploadInBackground(transfer: transfer, media: source)
  }

  private func renew(_ current: TeraSubmissionStatus, source: TeraPreparedMedia, using media: any TeraAddMediaHandling) async throws -> TeraSubmissionStatus {
    let renewed = if let stopControl {
      try await stopControl.renew(using: media, submission: current, source: source, client: client)
    } else {
      try await media.renewSubmissionUpload(current, media: source, client: client)
    }
    try accept(renewed)
    try ensure()
    return renewed
  }

  func reconcileStopped(_ initial: TeraSubmissionStatus) async throws {
    guard let media else { return }
    // The authoritative inventory carries each original revision and attempt
    // through idempotent completion. Never relabel it as the current head.
    _ = try await media.recoverNativeUploads(client: client)
    try ensure()
    try await accept(client.submissionStatus(request: initial.request))
  }
}
