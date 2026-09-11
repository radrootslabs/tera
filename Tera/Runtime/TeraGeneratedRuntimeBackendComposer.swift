import Foundation
import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  func reserveComposerID() async throws -> String {
    do { return try TeraGeneratedComposer.reserveID() } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func saveComposer(request: TeraComposerSaveRequest) async throws -> TeraComposerSaveReceipt {
    do { return try await TeraGeneratedComposer.save(runtime: runtime, request: request) } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func loadComposer(scope: TeraComposerScope, id: String) async throws -> TeraComposerDraft {
    do { return try await TeraGeneratedComposer.load(runtime: runtime, scope: scope, id: id) } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func listComposers(scope: TeraComposerScope, limit: UInt16, cursor: String?) async throws -> TeraComposerPage {
    do { return try await TeraGeneratedComposer.list(runtime: runtime, scope: scope, limit: limit, cursor: cursor) } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func reserveSubmissionID() async throws -> String {
    do { return try TeraGeneratedSubmission.reserveID() } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func reserveSubmission(request: TeraSubmissionRequest) async throws -> TeraSubmissionReservation {
    do { return try await TeraGeneratedSubmission.reserve(runtime: runtime, request: request) } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }
}
