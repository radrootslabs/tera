import Foundation

/// Bounded native scheduling over one immutable Rust-owned operation.
@MainActor
struct TeraSubmissionEffects {
  let client: TeraRuntimeClient
  let media: (any TeraAddMediaHandling)?
  let ensure: () throws -> Void
  let accept: (TeraSubmissionStatus) throws -> Void

  func advance(_ initial: TeraSubmissionStatus) async throws {
    try ensure()
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
    if ![.complete, .terminal, .cancelled].contains(current.state) {
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
      guard let source = prepared.first(where: { $0.opaqueReference == item.opaqueReference }),
            let handle = opened.handles.first(where: { $0.media.opaqueReference == item.opaqueReference })
      else {
        throw TeraComposerAcknowledgment.unconfirmed
      }
      let job = try await client.prepareSubmissionUpload(input: TeraSubmissionMediaRequest(
        request: current.request, expectedRevision: current.revision, media: handle
      ))
      try accept(job.submission)
      let receipt = try await media.uploadInBackground(transfer: job.transfer, media: source)
      try ensure()
      // Keep an uncertain OS receipt until Rust has durably verified it. A
      // storage/read/cancellation failure is never proof the upload was rejected.
      current = try await client.completeSubmissionUpload(input: TeraSubmissionMediaRequest(
        request: current.request, expectedRevision: job.submission.revision, media: handle
      ), response: receipt)
      try accept(current)
      try await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: true)
      try ensure()
    }
    return current
  }
}
