import Foundation

enum TeraAddLoadState: Sendable, Equatable {
  case idle
  case loading
  case ready
  case failed(String)
}

enum TeraComposerSaveState: Equatable {
  case idle
  case unsaved
  case saving
  case saved
  case failed
  case revision

  var label: String {
    switch self {
    case .idle: "Edits save on this device."
    case .unsaved: "Changes have not been saved."
    case .saving: "Saving changes on this device…"
    case .saved: "Changes saved on this device."
    case .failed: "Changes are not confirmed saved. Use Save draft to retry."
    case .revision: "Use Save draft to save this revision before leaving."
    }
  }
}

struct TeraComposerPersistence: Sendable {
  var reserve: @Sendable () async throws -> String
  var save: @Sendable (TeraComposerSaveRequest) async throws -> TeraComposerSaveReceipt
  var load: @Sendable (TeraComposerScope, String) async throws -> TeraComposerDraft

  init(client: TeraRuntimeClient) {
    reserve = { try await client.reserveComposerID() }
    save = { try await client.saveComposer(request: $0) }
    load = { try await client.loadComposer(scope: $0, id: $1) }
  }

  init(
    reserve: @escaping @Sendable () async throws -> String,
    save: @escaping @Sendable (TeraComposerSaveRequest) async throws -> TeraComposerSaveReceipt,
    load: @escaping @Sendable (TeraComposerScope, String) async throws -> TeraComposerDraft
  ) {
    self.reserve = reserve
    self.save = save
    self.load = load
  }
}

enum TeraComposerAcknowledgment {
  static func matches(_ draft: TeraComposerDraft, request: TeraComposerSaveRequest) -> Bool {
    let (revision, overflow) = (request.expectedRevision ?? 0).addingReportingOverflow(1)
    return !overflow && draft.scope == request.scope && draft.id == request.id
      && draft.revision == revision && draft.editSequence == request.editSequence && draft.form == request.form
  }

  static var unconfirmed: TeraRuntimeFailure {
    .local(operation: "add.composer.save", code: "ios.composer.save_unconfirmed",
           safeMessage: "These changes are not confirmed saved. Keep this draft open and retry Save.")
  }
}
