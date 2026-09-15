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
      try await media?.reconcileBackgroundSubmissions([initial])
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
    try await media?.reconcileBackgroundSubmissions([current])
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
      let receipt: TeraAddBackgroundUploadReceipt = if let stopControl {
        try await stopControl.upload(using: media, transfer: job.transfer, source: source)
      } else {
        try await media.uploadInBackground(transfer: job.transfer, media: source)
      }
      try ensure()
      // Keep an uncertain OS receipt until Rust has durably verified it. A
      // storage/read/cancellation failure is never proof the upload was rejected.
      current = try await client.completeSubmissionUpload(input: TeraSubmissionMediaRequest(
        request: current.request, expectedRevision: job.submission.revision, media: handle
      ), response: receipt)
      try accept(current)
      try await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: true)
      try ensure()
      if current.delivery.isStopped || !mayStart() {
        return current
      }
    }
    return current
  }

  func reconcileStopped(_ initial: TeraSubmissionStatus) async throws {
    guard let media else { return }
    var current = initial
    for source in initial.preparedMedia {
      try ensure()
      guard current.media.contains(where: { $0.opaqueReference == source.opaqueReference && $0.progress.stage == .uploading }),
            let receipt = try await media.retainedSubmissionUpload(current, media: source) else { continue }
      let opened = try await TeraOpenedMedia.open([source], using: media)
      defer { opened.close() }
      guard let handle = opened.handles.first else { throw TeraComposerAcknowledgment.unconfirmed }
      current = try await client.completeSubmissionUpload(input: TeraSubmissionMediaRequest(
        request: current.request, expectedRevision: current.revision, media: handle
      ), response: receipt)
      try accept(current)
      try await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: true)
    }
    // Pending, missing or unreadable native responses remain uncertain.
  }
}
