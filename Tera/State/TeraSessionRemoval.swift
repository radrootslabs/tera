import Foundation

extension TeraSessionStore {
  func applySettingsReconfiguration() async -> TeraSessionPhase {
    guard !removalInProgress else { return phase }
    generation = generation.invalidated()
    let requestedGeneration = generation
    phase = .starting
    do {
      let identity = try await identityStore.loadAndMigrate()
      try ensureCurrent(requestedGeneration)
      guard identity.state == .unlocked else {
        return await start()
      }
      let configuration = try await configurationStore.load()
        try ensureCurrent(requestedGeneration)
      phase = try await startRuntime(
        configuration: configuration,
        identity: identity,
        generation: requestedGeneration.requireActive(),
        forceReconfiguration: true,
        adoptBootstrapSettings: false
      )
    } catch {
      guard generation == requestedGeneration else { return phase }
      phase = .failed(
        .local(
          operation: "session.settings_reconfiguration",
          code: "ios.session.settings_reconfiguration_failed",
          safeMessage: "Tera could not apply the saved settings."
        )
      )
    }
    return phase
  }

  func keyRemovalPage(author: String, cursor: String?) async throws -> TeraLegacyDraftPage {
    guard !removalInProgress, case let .running(snapshot) = phase,
          snapshot.identity.publicKeyHex == author else { throw TeraIdentityStoreError.unavailable }
    let page = try await runtimeClient.legacyDraftPage(limit: 50, cursor: cursor)
    guard page.authorPublicKey == author else { throw TeraIdentityStoreError.unavailable }
    return page
  }

  func removeSigningKey(author: String, requests: [TeraKeyRemovalRequest]) async -> TeraSessionPhase {
    guard !removalInProgress, requests.count <= 100,
          case let .running(snapshot) = phase, snapshot.identity.publicKeyHex == author
    else { return phase }
    removalInProgress = true
    defer { removalInProgress = false }
    generation = generation.invalidated()
    let requestedGeneration = generation
    do {
      try ensureCurrent(requestedGeneration)
      let identity = try await identityStore.loadAndMigrate()
      guard identity.state == .unlocked, identity.publicKeyHex == author else {
        throw TeraIdentityStoreError.unavailable
      }
      for request in requests {
        try ensureCurrent(requestedGeneration)
        try await runtimeClient.prepareRetractionForKeyRemoval(request)
      }
      try ensureCurrent(requestedGeneration)
      _ = try await runtimeClient.stop()
      try ensureCurrent(requestedGeneration)
      let removed = try await identityStore.removeSigningKey(expected: identity)
      // Do not discard an actual completed deletion because cancellation or
      // lifecycle invalidation arrived after the last reversible boundary.
      if removed.state == .absent {
        phase = .identityRequired
      }
    } catch {
      // All local data and already signed requests remain with their owner.
      // Re-read custody on retry, including a recoverable partial transaction.
      guard generation == requestedGeneration else { return phase }
      phase = .failed(.local(operation: "identity.key_removal", code: "ios.identity.key_removal_incomplete",
                             safeMessage: "Key removal did not finish. Local data and any signed requests are retained. Retry to check identity recovery before continuing."))
    }
    return phase
  }

  func stop() async -> TeraSessionPhase {
    generation = generation.invalidated()
    let requestedGeneration = generation
    do {
      _ = try await runtimeClient.stop()
      try ensureCurrent(requestedGeneration)
      await identityStore.lock()
      try ensureCurrent(requestedGeneration)
      try qualificationEvidenceStore?.cleanup()
      phase = .stopped
    } catch let TeraRuntimeClientError.shutdown(failure) {
      guard isCurrent(requestedGeneration) else { return phase }
      phase = .failed(failure)
    } catch {
      guard isCurrent(requestedGeneration) else { return phase }
      phase = .failed(
        .local(
          operation: "session.stop",
          code: "ios.session.stop_failed",
          safeMessage: "Tera could not finish shutting down."
        )
      )
    }
    return phase
  }
}
