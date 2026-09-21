extension TeraRuntimeClient {
  /// Run the native inactivity reservation inside the retained runtime worker.
  /// A caller deadline cannot release it while the underlying FFI is returning.
  func withUploadRenewal(_ body: @escaping @Sendable (any TeraRuntimeBackend) async throws -> TeraSubmissionStatus) async throws -> TeraSubmissionStatus {
    try await addOperation("runtime.submission.renew_upload", submission: true, body)
  }

  func uploadSubmissionMedia(input: TeraSubmissionMediaRequest) async throws -> TeraSubmissionStatus {
    try await addOperation("runtime.submission.upload", submission: true) { try await $0.uploadSubmissionMedia(input: input) }
  }

  func requestSubmissionStop(request: TeraSubmissionRequest) async throws -> TeraSubmissionStatus {
    try await addOperation("runtime.submission.stop") { try await $0.requestSubmissionStop(request: request) }
  }

  func reconcileSubmissionLocal(request: TeraSubmissionRequest, context: TeraLocalNetwork) async throws -> TeraSubmissionStatus {
    try await addOperation("runtime.submission.reconcile_local", submission: true) {
      try await $0.reconcileSubmissionLocal(request: request, context: context)
    }
  }

  func prepareSubmission(request: TeraSubmissionRequest, media: [TeraPreparedMediaHandle]) async throws -> TeraSubmissionStatus {
    try await addOperation("runtime.submission.prepare", submission: true) { try await $0.prepareSubmission(request: request, media: media) }
  }

  func recoverSubmission(request: TeraSubmissionRequest) async throws -> TeraSubmissionStatus? {
    try await addOperation("runtime.submission.recover", submission: true) { try await $0.recoverSubmission(request: request) }
  }

  func submissionStatus(request: TeraSubmissionRequest) async throws -> TeraSubmissionStatus {
    try await addOperation("runtime.submission.status") { try await $0.submissionStatus(request: request) }
  }

  func advanceSubmission(request: TeraSubmissionRequest, expectedRevision: UInt64) async throws -> TeraSubmissionStatus {
    try await addOperation("runtime.submission.advance", submission: true) { try await $0.advanceSubmission(request: request, expectedRevision: expectedRevision) }
  }

  func listSubmissions(scope: TeraComposerScope, limit: UInt16 = 100, cursor: String? = nil) async throws -> TeraSubmissionPage {
    try await addOperation("runtime.submission.list") { try await $0.listSubmissions(scope: scope, limit: limit, cursor: cursor) }
  }

  func prepareSubmissionUpload(input: TeraSubmissionMediaRequest) async throws -> TeraSubmissionUploadJob {
    try await addOperation("runtime.submission.prepare_upload", submission: true) { try await $0.prepareSubmissionUpload(input: input) }
  }

  func completeSubmissionUpload(input: TeraSubmissionMediaRequest, response: TeraAddBackgroundUploadReceipt) async throws -> TeraSubmissionStatus {
    try await addOperation("runtime.submission.complete_upload", submission: true) { try await $0.completeSubmissionUpload(input: input, response: response) }
  }
}

extension TeraRuntimeBackend {
  func renewSubmissionUpload(input _: TeraSubmissionMediaRequest, renewal _: TeraSubmissionUploadRenewal) async throws -> TeraSubmissionUploadJob {
    throw submissionUnavailable()
  }

  func renewSubmissionMedia(input _: TeraSubmissionMediaRequest, renewal _: TeraSubmissionUploadRenewal) async throws -> TeraSubmissionStatus {
    throw submissionUnavailable()
  }

  func uploadSubmissionMedia(input _: TeraSubmissionMediaRequest) async throws -> TeraSubmissionStatus {
    throw submissionUnavailable()
  }

  func requestSubmissionStop(request _: TeraSubmissionRequest) async throws -> TeraSubmissionStatus {
    throw submissionUnavailable()
  }

  func reconcileSubmissionLocal(request _: TeraSubmissionRequest, context _: TeraLocalNetwork) async throws -> TeraSubmissionStatus {
    throw submissionUnavailable()
  }

  func prepareSubmission(request _: TeraSubmissionRequest, media _: [TeraPreparedMediaHandle]) async throws -> TeraSubmissionStatus {
    throw submissionUnavailable()
  }

  func recoverSubmission(request _: TeraSubmissionRequest) async throws -> TeraSubmissionStatus? {
    throw submissionUnavailable()
  }

  func submissionStatus(request _: TeraSubmissionRequest) async throws -> TeraSubmissionStatus {
    throw submissionUnavailable()
  }

  func advanceSubmission(request _: TeraSubmissionRequest, expectedRevision _: UInt64) async throws -> TeraSubmissionStatus {
    throw submissionUnavailable()
  }

  func listSubmissions(scope _: TeraComposerScope, limit _: UInt16, cursor _: String?) async throws -> TeraSubmissionPage {
    throw submissionUnavailable()
  }

  func prepareSubmissionUpload(input _: TeraSubmissionMediaRequest) async throws -> TeraSubmissionUploadJob {
    throw submissionUnavailable()
  }

  func completeSubmissionUpload(input _: TeraSubmissionMediaRequest, response _: TeraAddBackgroundUploadReceipt) async throws -> TeraSubmissionStatus {
    throw submissionUnavailable()
  }

  private func submissionUnavailable() -> TeraRuntimeFailure {
    .local(operation: "runtime.submission", code: "ios.add.unsupported", safeMessage: "Scoped submission is unavailable.")
  }
}
