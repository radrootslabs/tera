import TeraKitBindings

extension FfiRuntimeChangeRecord {
  var appValue: TeraRuntimeChange {
    TeraRuntimeChange(
      schemaVersion: schemaVersion,
      scope: TeraRuntimeChangeScope(
        publicKey: scope.publicKey,
        sourceGeneration: scope.sourceGeneration,
        context: scope.context.map {
          TeraLocalNetwork(
            schemaVersion: $0.schemaVersion, id: $0.id, label: $0.label,
            relayURLs: $0.relayUrls, locality: $0.locality,
            followedAuthors: $0.followedAuthors, generation: $0.generation
          )
        }
      ),
      epoch: epoch, revision: revision.appValue, kind: kind.appValue, entityID: entityId
    )
  }
}

extension FfiInvalidationRevision {
  fileprivate var appValue: TeraProjectionRevision {
    switch self {
    case let .current(value): TeraProjectionRevision(rawValue: value)
    case .exhausted: .exhausted
    }
  }
}

extension FfiRuntimeChangeKind {
  fileprivate var appValue: TeraRuntimeChangeKind {
    switch self {
    case .initial: .initial
    case .identity: .identity
    case .settings: .settings
    case .profile: .profile
    case .today: .today
    case .drafts: .drafts
    case .relay: .relay
    case .media: .media
    case .lifecycle: .lifecycle
    }
  }
}
