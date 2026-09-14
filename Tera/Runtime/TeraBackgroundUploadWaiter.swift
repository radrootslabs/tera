import Foundation
import RadrootsKit

/// Exact retained-request discovery and cancellation-aware response waiting.
enum TeraBackgroundUploadWaiter {
  static func matchingPersistedUpload(
    transfer: any RadrootsBackgroundTransfer,
    draftID: String,
    expectedRevision: UInt64,
    request: RadrootsBackgroundTransferRequest
  ) async throws -> RadrootsBackgroundTransferSnapshot? {
    try Task.checkCancellation()
    let snapshots = try await transfer.snapshots()
    let prefix = "radroots.add.\(draftID)."
    let owned = snapshots.filter { $0.identifier.rawValue.hasPrefix(prefix) }
    let parsed = try owned.map { snapshot in
      guard let identity = TeraBackgroundUploadRequest.transferIdentity(snapshot.identifier),
        identity.draftID == draftID,
        identity.revision <= expectedRevision
      else {
        throw Self.failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload identity is invalid."
        )
      }
      return snapshot
    }
    let candidates = parsed.filter { snapshot in
      snapshot.state != .completed
        || TeraBackgroundUploadRequest.persistedRequestMatches(snapshot.request, request: request)
    }
    try Task.checkCancellation()
    let active = candidates.filter { $0.state != .completed }
    guard active.count <= 1 else {
      throw Self.failure(
        code: "ios.add.background_upload_ambiguous",
        message: "The persisted photo upload state is ambiguous."
      )
    }
    if let candidate = active.first {
      guard TeraBackgroundUploadRequest.persistedRequestMatches(candidate.request, request: request) else {
        throw Self.failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload does not match the authorized upload."
        )
      }
      return candidate
    }
    let completed = candidates.filter {
      $0.state == .completed && TeraBackgroundUploadRequest.persistedRequestMatches($0.request, request: request)
    }
    guard completed.count <= 1 else {
      throw Self.failure(
        code: "ios.add.background_upload_ambiguous",
        message: "The persisted photo upload state is ambiguous."
      )
    }
    return completed.first
  }

  static func receipt(
    transfer: any RadrootsBackgroundTransfer,
    for identifier: RadrootsBackgroundTransferIdentifier,
    draftID: String,
    expectedRevision: UInt64,
    request: RadrootsBackgroundTransferRequest
  ) async throws -> TeraAddBackgroundUploadReceipt {
    while true {
      try Task.checkCancellation()
      guard let snapshot = try await transfer.snapshot(for: identifier) else {
        throw failure(
          code: "ios.add.background_upload_missing",
          message: "The background photo upload could not be recovered."
        )
      }
      guard TeraBackgroundUploadRequest.persistedRequestMatches(snapshot.request, request: request) else {
        throw Self.failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload no longer matches its request."
        )
      }
      switch snapshot.state {
      case .awaitingVerification, .completed:
        try Task.checkCancellation()
        guard let response = snapshot.response,
          let statusCode = UInt16(exactly: response.statusCode),
          let body = response.body
        else {
          throw Self.failure(
            code: "ios.add.background_response_invalid",
            message: "The photo service returned an invalid response."
          )
        }
        return TeraAddBackgroundUploadReceipt(
          identifier: identifier.rawValue,
          draftID: draftID,
          expectedRevision: expectedRevision,
          statusCode: statusCode,
          mediaType: response.mediaType,
          contentEncoding: response.contentEncoding,
          body: body
        )
      case .failed, .interrupted, .cancelled, .expired:
        throw Self.failure(
          code: snapshot.failure?.rawValue ?? "ios.add.background_upload_failed",
          message: TeraUserMessages.text(.backgroundTransferFailed)
        )
      case .queued, .running:
        try await Task.sleep(for: .milliseconds(100))
      }
    }
  }

  private static func failure(code: String, message: String) -> TeraRuntimeFailure {
    .local(operation: "add.media.background", code: code, safeMessage: message)
  }
}
