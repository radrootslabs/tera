import Foundation

enum TeraRecoveryOwner: Sendable, Equatable {
  case legacy
  case submission(TeraSubmissionRequest)
  case repair
}

struct TeraRecoveryEntry: Sendable, Equatable {
  let key: String
  let revision: UInt64
  let owner: TeraRecoveryOwner
}

struct TeraRecoveryPage: Sendable, Equatable {
  let author: String
  let entries: [TeraRecoveryEntry]
  let scanned: UInt16
  let nextCursor: String?
}

struct TeraNativeRecoveryProgress: Sendable, Equatable {
  let visited: Int
  let remaining: Int
  let needsAttention: Bool
  var issues: [TeraNativeRecoveryIssue] = []
  var pause: TeraNativeRecoveryPause?
}

extension TeraRuntimeClient {
  func recoveryPage(limit: UInt16 = 32, cursor: String? = nil) async throws -> TeraRecoveryPage {
    try await addOperation("runtime.recovery.page") { try await $0.recoveryPage(limit: limit, cursor: cursor) }
  }

  /// Keep exact identity selection and full status lookup in one admitted runtime
  /// operation. A displayed page supplies no ownership or completeness evidence.
  func recoveryUploadOwner(key: String) async throws -> TeraNativeUploadRecoveryOwner? {
    try await addOperation("runtime.recovery.parent") { backend in
      guard let entry = try await backend.recoveryParent(key: key) else { return nil }
      guard entry.key == key else { throw TeraComposerAcknowledgment.unconfirmed }
      switch entry.owner {
      case .legacy:
        let value = try await backend.draftStatus(id: key)
        guard value.id == key, value.revision >= entry.revision else { throw TeraComposerAcknowledgment.unconfirmed }
        return TeraNativeUploadRecoveryOwner(draft: value)
      case let .submission(request):
        let value = try await backend.submissionStatus(request: request)
        guard value.intentID == key, value.request == request, value.revision >= entry.revision else { throw TeraComposerAcknowledgment.unconfirmed }
        return TeraNativeUploadRecoveryOwner(submission: value)
      case .repair: throw TeraNativeRecoveryFault.invalidParent
      }
    }
  }
}

extension TeraRuntimeBackend {
  func recoveryPage(limit _: UInt16, cursor _: String?) async throws -> TeraRecoveryPage {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func recoveryParent(key _: String) async throws -> TeraRecoveryEntry? {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}

extension TeraNativeUploadRecoveryOwner {
  init(draft: TeraDraftStatus) {
    self.init(id: draft.id, revision: draft.revision, media: draft.form?.media ?? [],
              verifiedURLs: Set(draft.media.filter { $0.stage == .verified }.map(\.url)),
              uploadURLs: Self.urls(draft.media))
  }

  init(submission: TeraSubmissionStatus) {
    self.init(id: submission.intentID, revision: submission.revision, media: submission.preparedMedia,
              verifiedURLs: Set(submission.media.filter { $0.progress.stage == .verified }.map(\.progress.url)),
              uploadURLs: Self.urls(submission.media.map(\.progress)))
  }

  private static func urls(_ media: [TeraDraftMediaStatus]) -> [String: String] {
    media.reduce(into: [:]) { urls, item in
      if let url = item.uploadURL {
        urls[item.url] = url
      }
    }
  }
}
