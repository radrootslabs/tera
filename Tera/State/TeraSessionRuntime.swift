import Foundation
import RadrootsKit

extension TeraSessionStore {
  func startRuntime(
    configuration: TeraAppConfiguration,
    identity: TeraAppIdentity,
    generation requestedGeneration: TeraSessionGeneration,
    forceReconfiguration: Bool,
    adoptBootstrapSettings: Bool
  ) async throws -> TeraSessionPhase {
    guard let publicKeyHex = identity.publicKeyHex,
      let signerGeneration = identity.signerGeneration
    else {
      throw TeraIdentityStoreError.unavailable
    }
    let mobileStore = try RadrootsAppleMobileStore.prepare(
      roots: roots,
      publicKeyHex: publicKeyHex,
      protectedDataAvailability: protectedData.isAvailable() ? .available : .unavailable
    )
    let sourceGeneration = try await configurationStore.sourceGeneration()
    try ensureCurrent(requestedGeneration)
    let signer = try await identityStore.signer(for: identity)
    guard generation == requestedGeneration else { throw TeraRuntimeClientError.superseded }
    let launchConfiguration = TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: mobileStore.applicationSupportDirectory.path,
      publicKeyHex: publicKeyHex,
      sourceGenerationHex: sourceGeneration.generationHex,
      sourceGenerationCreatedAtUnixMilliseconds: sourceGeneration.createdAtUnixMilliseconds,
      protectedData: protectedData.isAvailable() ? .available : .unavailable,
      networkProfile: configuration.profile.runtimeValue,
      writableRelays: configuration.writableRelays,
      blossom: configuration.blossom,
      app: configuration.appMetadata,
      signerGeneration: signerGeneration,
      signer: signer,
      adoptBootstrapSettings: adoptBootstrapSettings
    )
    let snapshot =
      if forceReconfiguration {
        try await runtimeClient.reconfigure(configuration: launchConfiguration)
      } else {
        try await runtimeClient.start(configuration: launchConfiguration)
      }
    guard generation == requestedGeneration else {
      throw TeraRuntimeClientError.superseded
    }
    try await reconcileIdentity(identity, generation: requestedGeneration)
    try ensureCurrent(requestedGeneration)
    if configuration.activationState == .reconfigurationRequired,
      adoptBootstrapSettings
    {
      try await configurationStore.confirmBootstrapActivation(
        expectedGeneration: configuration.generation
      )
    }
    guard generation == requestedGeneration else {
      throw TeraRuntimeClientError.superseded
    }
    return .running(snapshot)
  }

  private func reconcileIdentity(
    _ identity: TeraAppIdentity, generation requestedGeneration: TeraSessionGeneration
  ) async throws {
    guard let identityID = identity.identityHandle,
      let publicKeyHex = identity.publicKeyHex
    else {
      throw TeraIdentityStoreError.unavailable
    }
    var settings = try await runtimeClient.mobileSettings()
    try ensureCurrent(requestedGeneration)
    if let pending = settings.identity.pendingImportOperationID {
      settings = try await runtimeClient.applyIdentityCommand(
        expectedRevision: settings.revision,
        command: TeraIdentityCommand(
          kind: .cancelImport,
          operationID: pending,
          identityID: nil,
          publicKeyHex: nil
        )
      ).settings
      try ensureCurrent(requestedGeneration)
    }
    settings = try await adoptIdentity(
      identityID: identityID, publicKeyHex: publicKeyHex, settings: settings, generation: requestedGeneration
    )
    try ensureCurrent(requestedGeneration)
    _ = try await runtimeClient.applyIdentityCommand(
      expectedRevision: settings.revision,
      command: TeraIdentityCommand(
        kind: .unlock,
        operationID: nil,
        identityID: nil,
        publicKeyHex: nil
      )
    )
    try ensureCurrent(requestedGeneration)
  }

  private func adoptIdentity(
    identityID: String, publicKeyHex: String, settings initial: TeraMobileSettings,
    generation requestedGeneration: TeraSessionGeneration
  ) async throws -> TeraMobileSettings {
    var settings = initial
    if let existing = settings.identity.identities.first(where: {
      $0.publicKeyHex == publicKeyHex
    }) {
      if settings.identity.activeIdentityID != existing.id {
        settings = try await runtimeClient.applyIdentityCommand(
          expectedRevision: settings.revision,
          command: TeraIdentityCommand(
            kind: .select,
            operationID: nil,
            identityID: existing.id,
            publicKeyHex: nil
          )
        ).settings
      try ensureCurrent(requestedGeneration)
      }
    } else {
      let operationID = UUID().uuidString.lowercased()
      settings = try await runtimeClient.applyIdentityCommand(
        expectedRevision: settings.revision,
        command: TeraIdentityCommand(
          kind: .beginImport,
          operationID: operationID,
          identityID: nil,
          publicKeyHex: nil
        )
      ).settings
      try ensureCurrent(requestedGeneration)
      settings = try await runtimeClient.applyIdentityCommand(
        expectedRevision: settings.revision,
        command: TeraIdentityCommand(
          kind: .completeImport,
          operationID: operationID,
          identityID: identityID,
          publicKeyHex: publicKeyHex
        )
      ).settings
      try ensureCurrent(requestedGeneration)
    }
    return settings
  }
}
