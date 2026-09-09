import Foundation
import RadrootsKit

final class TeraProtectedDataMonitor: @unchecked Sendable {
  private let lock = NSLock()
  private var available: Bool

  init(available: Bool) {
    self.available = available
  }

  func update(available: Bool) {
    lock.withLock { self.available = available }
  }

  func isAvailable() -> Bool {
    lock.withLock { available }
  }
}

enum TeraSessionPhase: Sendable, Equatable {
  case starting
  case identityRequired
  case identityLocked(TeraAppIdentity)
  case protectedDataUnavailable(TeraAppIdentity)
  case recoveryRequired(TeraAppIdentity)
  case corruptIdentity(TeraAppIdentity)
  case configurationReconfigurationRequired(TeraConfigurationReconfigurationRequirement)
  case running(TeraRuntimeSnapshot)
  case stopped
  case failed(TeraRuntimeFailure)
}

struct TeraConfigurationReconfigurationRequirement: Sendable, Equatable {
  let generation: UInt64
  let previousBlossomConfigFingerprint: String?
}

actor TeraSessionStore {
  private let configurationStore: TeraConfigurationStore
  private let identityStore: TeraIdentityStore
  private let runtimeClient: TeraRuntimeClient
  private let roots: RadrootsAppleFileRoots
  private let protectedData: TeraProtectedDataMonitor
  private let automatesQualificationIdentity: Bool
  private let qualificationEvidenceStore: TeraRemoteQualificationEvidenceStore?
  private var generation = TeraSessionGeneration.initial
  private var phase: TeraSessionPhase = .starting

  init(
    configurationStore: TeraConfigurationStore,
    identityStore: TeraIdentityStore,
    runtimeClient: TeraRuntimeClient,
    roots: RadrootsAppleFileRoots,
    protectedData: TeraProtectedDataMonitor,
    automatesQualificationIdentity: Bool = false,
    qualificationEvidenceStore: TeraRemoteQualificationEvidenceStore? = nil
  ) {
    self.configurationStore = configurationStore
    self.identityStore = identityStore
    self.runtimeClient = runtimeClient
    self.roots = roots
    self.protectedData = protectedData
    self.automatesQualificationIdentity = automatesQualificationIdentity
    self.qualificationEvidenceStore = qualificationEvidenceStore
  }

  func updateProtectedDataAvailability(_ available: Bool) {
    protectedData.update(available: available)
  }

  func currentPhase() -> TeraSessionPhase {
    phase
  }

  func start() async -> TeraSessionPhase {
    await start(acceptingReconfiguration: false)
  }

  func applyConfigurationReconfiguration() async -> TeraSessionPhase {
    await start(acceptingReconfiguration: true)
  }

  func applySettingsReconfiguration() async -> TeraSessionPhase {
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

  func suspend() async {
    generation = generation.invalidated()
    let requestedGeneration = generation
    await runtimeClient.suspend()
    if generation == requestedGeneration, case .starting = phase {
      phase = .stopped
    }
  }

  private func start(acceptingReconfiguration: Bool) async -> TeraSessionPhase {
    generation = generation.invalidated()
    let requestedGeneration = generation
    phase = .starting
    do {
      let identity = try await identityStore.loadAndMigrate()
      guard generation == requestedGeneration else {
        throw TeraRuntimeClientError.superseded
      }
      switch identity.state {
      case .absent:
        phase = .identityRequired
      case .locked:
        #if DEBUG
          if automatesQualificationIdentity {
            return await unlockIdentity()
          }
        #endif
        phase = .identityLocked(identity)
      case .protectedDataUnavailable:
        phase = .protectedDataUnavailable(identity)
      case .recoveryRequired:
        phase = .recoveryRequired(identity)
      case .corrupt:
        phase = .corruptIdentity(identity)
      case .unlocked:
        phase = try await startUnlocked(
          identity, acceptingReconfiguration: acceptingReconfiguration, generation: requestedGeneration
        )
      }
    } catch TeraRuntimeClientError.superseded {
      return phase
    } catch let TeraRuntimeClientError.startup(failure) {
      guard generation == requestedGeneration else { return phase }
      phase = .failed(failure)
    } catch {
      guard generation == requestedGeneration else { return phase }
      phase = .failed(
        .local(
          operation: "session.start",
          code: "ios.session.start_failed",
          safeMessage: TeraUserMessages.text(for: error, fallback: .startupFailed)
        )
      )
    }
    return phase
  }

  private func startUnlocked(
    _ identity: TeraAppIdentity, acceptingReconfiguration: Bool,
    generation requestedGeneration: TeraSessionGeneration
  ) async throws -> TeraSessionPhase {
    let configuration = try await configurationStore.load()
    try ensureCurrent(requestedGeneration)
    if configuration.activationState == .reconfigurationRequired,
      !acceptingReconfiguration
    {
      return .configurationReconfigurationRequired(
        TeraConfigurationReconfigurationRequirement(
          generation: configuration.generation,
          previousBlossomConfigFingerprint: configuration
            .previousBlossomConfigFingerprint
        )
      )
    }
    return try await startRuntime(
      configuration: configuration,
      identity: identity,
      generation: requestedGeneration.requireActive(),
      forceReconfiguration: configuration.activationState
        == .reconfigurationRequired,
      adoptBootstrapSettings: configuration.activationState
        == .reconfigurationRequired
    )
  }

  func createIdentity(label: String? = nil) async -> TeraSessionPhase {
    await runIdentityOperation { try await self.identityStore.create(label: label) }
  }

  func importIdentity(
    _ material: RadrootsIdentitySecretMaterial,
    label: String? = nil
  ) async -> TeraSessionPhase {
    await runIdentityOperation { try await self.identityStore.importIdentity(material, label: label) }
  }

  func lockIdentity() async -> TeraSessionPhase {
    generation = generation.invalidated()
    let requestedGeneration = generation
    if case .running = phase,
      let settings = try? await runtimeClient.mobileSettings()
    {
      guard isCurrent(requestedGeneration) else { return phase }
      _ = try? await runtimeClient.applyIdentityCommand(
        expectedRevision: settings.revision,
        command: TeraIdentityCommand(
          kind: .lock,
          operationID: nil,
          identityID: nil,
          publicKeyHex: nil
        )
      )
    }
    guard isCurrent(requestedGeneration) else { return phase }
    _ = try? await runtimeClient.stop()
    guard isCurrent(requestedGeneration) else { return phase }
    await identityStore.lock()
    guard isCurrent(requestedGeneration) else { return phase }
    let identity = await identityStore.snapshot()
    guard isCurrent(requestedGeneration) else { return phase }
    phase = .identityLocked(identity)
    return phase
  }

  func unlockIdentity() async -> TeraSessionPhase {
    await runIdentityOperation { try await self.identityStore.unlock() }
  }

  func recoverIdentity() async -> TeraSessionPhase {
    await runIdentityOperation { try await self.identityStore.recover() }
  }

  private func runIdentityOperation(
    _ operation: () async throws -> TeraAppIdentity
  ) async -> TeraSessionPhase {
    generation = generation.invalidated()
    let requestedGeneration = generation
    do {
      try ensureCurrent(requestedGeneration)
      _ = try await operation()
      try ensureCurrent(requestedGeneration)
      return await start()
    } catch {
      guard generation == requestedGeneration, !Task.isCancelled else { return phase }
      return failIdentityOperation(error)
    }
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

  private func startRuntime(
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

  private func isCurrent(_ requested: TeraSessionGeneration) -> Bool {
    generation == requested && generation.isActive && !Task.isCancelled
  }

  private func ensureCurrent(_ requested: TeraSessionGeneration) throws {
    guard isCurrent(requested) else { throw TeraRuntimeClientError.superseded }
  }

  private func failIdentityOperation(_ error: Error) -> TeraSessionPhase {
    generation = generation.invalidated()
    phase = .failed(
      .local(
        operation: "identity.operation",
        code: "ios.identity.operation_failed",
        safeMessage: TeraUserMessages.text(for: error, fallback: .identityOperationFailed)
      )
    )
    return phase
  }
}
