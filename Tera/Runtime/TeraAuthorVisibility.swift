import Foundation

enum TeraAuthorVisibility: String, Sendable, Equatable {
  case visible, muted, blocked
}

struct TeraAuthorVisibilityEntry: Sendable, Equatable, Identifiable, CustomDebugStringConvertible {
  let author: String
  let visibility: TeraAuthorVisibility
  var debugDescription: String {
    "AuthorVisibilityEntry"
  }

  var id: String {
    author
  }
}

struct TeraAuthorVisibilityPolicy: Sendable, Equatable, CustomDebugStringConvertible {
  let revision: UInt64
  let entries: [TeraAuthorVisibilityEntry]
  var debugDescription: String {
    "AuthorVisibilityPolicy(revision: \(revision), entries: \(entries.count))"
  }
}

extension TeraRuntimeBackend {
  func authorVisibility() async throws -> TeraAuthorVisibilityPolicy {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func setAuthorVisibility(author _: String, visibility _: TeraAuthorVisibility) async throws -> TeraAuthorVisibilityPolicy {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}

extension TeraRuntimeClient {
  func authorVisibility() async throws -> TeraAuthorVisibilityPolicy {
    try await supportOperation("runtime.visibility.read") { try await $0.authorVisibility() }
  }

  func setAuthorVisibility(author: String, visibility: TeraAuthorVisibility) async throws -> TeraAuthorVisibilityPolicy {
    try await supportOperation("runtime.visibility.change") { try await $0.setAuthorVisibility(author: author, visibility: visibility) }
  }
}
