import Foundation

/// Identity of one intentional action; retain the request unchanged for every retry.
struct TeraSubmissionRequest: Sendable, Equatable {
  let commandID: String
  let scope: TeraComposerScope
  let composerID: String
  let expectedRevision: UInt64
}

/// A saved source reservation, without publication or delivery authority.
struct TeraSubmissionReservation: Sendable, Equatable {
  let commandID: String
  let reservationID: String
  let captured: TeraComposerDraft
  let reservedAtUnixMilliseconds: UInt64
  let replayed: Bool
}

extension TeraRuntimeBackend {
  func reserveSubmissionID() async throws -> String {
    throw TeraRuntimeFailure.local(operation: "runtime.submission", code: "ios.add.unsupported",
                                   safeMessage: "Submission reservation is unavailable.")
  }

  func reserveSubmission(request _: TeraSubmissionRequest) async throws -> TeraSubmissionReservation {
    throw TeraRuntimeFailure.local(operation: "runtime.submission", code: "ios.add.unsupported",
                                   safeMessage: "Submission reservation is unavailable.")
  }
}

extension TeraRuntimeClient {
  func reserveSubmissionID() async throws -> String {
    try await addOperation("runtime.submission.reserve_id") { backend in
      try await backend.reserveSubmissionID()
    }
  }

  func reserveSubmission(request: TeraSubmissionRequest) async throws -> TeraSubmissionReservation {
    try await addOperation("runtime.submission.reserve") { backend in
      try await backend.reserveSubmission(request: request)
    }
  }
}
