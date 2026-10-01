import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  func authorVisibility() async throws -> TeraAuthorVisibilityPolicy {
    do { return try await runtime.authorVisibility().appValue } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func setAuthorVisibility(author: String, visibility: TeraAuthorVisibility) async throws -> TeraAuthorVisibilityPolicy {
    let mode: FfiAuthorVisibility = switch visibility {
    case .visible: .visible
    case .muted: .muted
    case .blocked: .blocked
    }
    do { return try await runtime.setAuthorVisibility(author: author, visibility: mode).appValue } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }
}

private extension FfiAuthorVisibilityPolicy {
  var appValue: TeraAuthorVisibilityPolicy {
    TeraAuthorVisibilityPolicy(revision: revision, entries: entries.map {
      let mode: TeraAuthorVisibility = switch $0.visibility {
      case .visible: .visible
      case .muted: .muted
      case .blocked: .blocked
      }
      return TeraAuthorVisibilityEntry(author: $0.author, visibility: mode)
    })
  }
}
