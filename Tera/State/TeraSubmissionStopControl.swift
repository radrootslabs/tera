import Foundation

/// Cancels only this submission's native upload waiter. Durable operation and
/// OS transfer evidence remain owned by Rust and the background transfer store.
@MainActor
final class TeraSubmissionStopControl {
  private(set) var requested = false
  private var uploadWaiter: Task<TeraAddBackgroundUploadReceipt, Error>?
  private var renewalWaiter: Task<TeraSubmissionStatus, Error>?

  func request() {
    requested = true
    uploadWaiter?.cancel()
    renewalWaiter?.cancel()
  }

  func reset() {
    uploadWaiter?.cancel()
    uploadWaiter = nil
    renewalWaiter?.cancel()
    renewalWaiter = nil
    requested = false
  }

  func renew(using media: any TeraAddMediaHandling, submission: TeraSubmissionStatus,
             source: TeraPreparedMedia, client: TeraRuntimeClient) async throws -> TeraSubmissionStatus
  {
    guard !requested, uploadWaiter == nil, renewalWaiter == nil else { throw CancellationError() }
    let task = Task { try await media.renewSubmissionUpload(submission, media: source, client: client) }
    renewalWaiter = task
    defer { renewalWaiter = nil }
    return try await withTaskCancellationHandler { try await task.value } onCancel: { task.cancel() }
  }

  func upload(using media: any TeraAddMediaHandling, transfer: TeraNativeTransferJob,
              source: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt
  {
    guard !requested, uploadWaiter == nil else { throw CancellationError() }
    let task = Task {
      try Task.checkCancellation()
      return try await media.uploadInBackground(transfer: transfer, media: source)
    }
    uploadWaiter = task
    defer { uploadWaiter = nil }
    return try await withTaskCancellationHandler {
      try await task.value
    } onCancel: {
      task.cancel()
    }
  }
}
