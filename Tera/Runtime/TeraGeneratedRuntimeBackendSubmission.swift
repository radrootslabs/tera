import Foundation
import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  func uploadSubmissionMedia(input: TeraSubmissionMediaRequest) async throws -> TeraSubmissionStatus {
    do {
      let value = try await runtime.submissionUploadMedia(input: input.generatedValue)
      return try TeraGeneratedSubmission.operation(value, expected: input.request)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func requestSubmissionStop(request: TeraSubmissionRequest) async throws -> TeraSubmissionStatus {
    do {
      let value = try await runtime.submissionRequestStop(request: request.generatedValue)
      return try TeraGeneratedSubmission.operation(value, expected: request)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func reconcileSubmissionLocal(request: TeraSubmissionRequest, context: TeraLocalNetwork) async throws -> TeraSubmissionStatus {
    do {
      let value = try await runtime.submissionReconcileLocal(request: request.generatedValue, context: context.generatedValue)
      return try TeraGeneratedSubmission.operation(value, expected: request)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func prepareSubmission(request: TeraSubmissionRequest, media: [TeraPreparedMediaHandle]) async throws -> TeraSubmissionStatus {
    do {
      let result = try await runtime.submissionPrepare(request: request.generatedValue, media: media.map(\.generatedValue))
      return try TeraGeneratedSubmission.operation(result, expected: request)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func recoverSubmission(request: TeraSubmissionRequest) async throws -> TeraSubmissionStatus? {
    do {
      let value = try await runtime.submissionRecover(request: request.generatedValue)
      return try value.map { try TeraGeneratedSubmission.operation($0, expected: request) }
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func submissionStatus(request: TeraSubmissionRequest) async throws -> TeraSubmissionStatus {
    do {
      let value = try await runtime.submissionStatus(request: request.generatedValue)
      return try TeraGeneratedSubmission.operation(value, expected: request)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func advanceSubmission(request: TeraSubmissionRequest, expectedRevision: UInt64) async throws -> TeraSubmissionStatus {
    do {
      let value = try await runtime.submissionAdvance(request: request.generatedValue, expectedRevision: expectedRevision)
      return try TeraGeneratedSubmission.operation(value, expected: request)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func listSubmissions(scope: TeraComposerScope, limit: UInt16, cursor: String?) async throws -> TeraSubmissionPage {
    do {
      let value = try await runtime.submissionPage(scope: scope.generatedValue, limit: limit, cursor: cursor)
      return try TeraGeneratedSubmission.page(value, scope: scope, limit: limit)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func prepareSubmissionUpload(input: TeraSubmissionMediaRequest) async throws -> TeraSubmissionUploadJob {
    do {
      let value = try await runtime.submissionPrepareUpload(input: input.generatedValue)
      return try TeraGeneratedSubmission.upload(value, input: input)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func completeSubmissionUpload(input: TeraSubmissionMediaRequest, response: TeraAddBackgroundUploadReceipt) async throws -> TeraSubmissionStatus {
    do {
      let current = try await submissionStatus(request: input.request)
      guard response.draftID == current.intentID, response.expectedRevision == input.expectedRevision,
            current.revision == input.expectedRevision else { throw TeraGeneratedSubmission.mismatch() }
      let value = try await runtime.submissionCompleteUpload(input: input.generatedValue, response: FfiSubmissionUploadResponse(
        schemaVersion: 1, statusCode: response.statusCode, mediaType: response.mediaType,
        contentEncoding: response.contentEncoding, body: response.body
      ))
      return try TeraGeneratedSubmission.operation(value, expected: input.request)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }
}
