import Foundation
import RadrootsKit
import UIKit

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

  @MainActor
  static func production(
    bundle: Bundle = .main,
    runtimeClient: TeraRuntimeClient = .production()
  ) throws -> TeraSessionStore {
    guard let bundleIdentifier = bundle.bundleIdentifier else {
      throw TeraConfigurationError.missing("bundle_identifier")
    }
    let qualification = try TeraRemoteQualificationEnvironment.current()
    #if DEBUG
      let qualificationEvidenceStore = try TeraRemoteQualificationEvidence.prepare()
    #else
      let qualificationEvidenceStore: TeraRemoteQualificationEvidenceStore? = nil
    #endif
    let servicePrefix =
      try qualification?.keychainServicePrefix
      ?? requiredString(
        "TERA_IOS_KEYCHAIN_SERVICE_PREFIX", bundle: bundle
      )
    let bootstrap = try TeraConfigurationBootstrap(
      runtimeMode: qualification?.runtimeMode
        ?? requiredString("TERA_IOS_RUNTIME_MODE", bundle: bundle),
      relayURLs: qualification?.relayURLs
        ?? array("TERA_IOS_NOSTR_RELAY_URLS", bundle: bundle),
      blossomOrigins: qualification?.blossomOrigins
        ?? array("TERA_IOS_BLOSSOM_ORIGINS", bundle: bundle),
      keychainServicePrefix: servicePrefix,
      bundleIdentifier: bundleIdentifier,
      appMetadata: TeraRuntimeAppMetadata(
        bundleIdentifier: bundleIdentifier,
        version: bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
          ?? "0",
        buildNumber: bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "0",
        buildSHA: normalizedOptional(
          bundle.object(forInfoDictionaryKey: "GIT_SHA") as? String
        )
      )
    )
    let roots = try TeraRemoteQualificationEnvironment.applicationFileRoots(
      appIdentifier: bundleIdentifier
    )
    let protectedData = TeraProtectedDataMonitor(
      available: UIApplication.shared.isProtectedDataAvailable
    )
    return try TeraSessionStore(
      configurationStore: TeraConfigurationStore(bootstrap: bootstrap, roots: roots),
      identityStore: .production(
        servicePrefix: servicePrefix,
        protectedDataAvailable: { protectedData.isAvailable() },
        qualification: qualification
      ),
      runtimeClient: runtimeClient,
      roots: roots,
      protectedData: protectedData,
      automatesQualificationIdentity: qualification?.automatesIdentity == true,
      qualificationEvidenceStore: qualificationEvidenceStore
    )
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
      guard identity.state == .unlocked else {
        return await start()
      }
      let configuration = try await configurationStore.load()
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
    await runtimeClient.suspend()
    if case .starting = phase {
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
        let configuration = try await configurationStore.load()
        if configuration.activationState == .reconfigurationRequired,
          !acceptingReconfiguration
        {
          phase = .configurationReconfigurationRequired(
            TeraConfigurationReconfigurationRequirement(
              generation: configuration.generation,
              previousBlossomConfigFingerprint: configuration
                .previousBlossomConfigFingerprint
            )
          )
          return phase
        }
        phase = try await startRuntime(
          configuration: configuration,
          identity: identity,
          generation: requestedGeneration.requireActive(),
          forceReconfiguration: configuration.activationState
            == .reconfigurationRequired,
          adoptBootstrapSettings: configuration.activationState
            == .reconfigurationRequired
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

  func createIdentity(label: String? = nil) async -> TeraSessionPhase {
    do {
      _ = try await identityStore.create(label: label)
    } catch {
      return failIdentityOperation(error)
    }
    return await start()
  }

  func importIdentity(
    _ material: RadrootsIdentitySecretMaterial,
    label: String? = nil
  ) async -> TeraSessionPhase {
    do {
      _ = try await identityStore.importIdentity(material, label: label)
    } catch {
      return failIdentityOperation(error)
    }
    return await start()
  }

  func lockIdentity() async -> TeraSessionPhase {
    generation = generation.invalidated()
    if case .running = phase,
      let settings = try? await runtimeClient.mobileSettings()
    {
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
    _ = try? await runtimeClient.stop()
    await identityStore.lock()
    let identity = await identityStore.snapshot()
    phase = .identityLocked(identity)
    return phase
  }

  func unlockIdentity() async -> TeraSessionPhase {
    do {
      _ = try await identityStore.unlock()
    } catch {
      return failIdentityOperation(error)
    }
    return await start()
  }

  func recoverIdentity() async -> TeraSessionPhase {
    do {
      _ = try await identityStore.recover()
    } catch {
      return failIdentityOperation(error)
    }
    return await start()
  }

  func stop() async -> TeraSessionPhase {
    generation = generation.invalidated()
    do {
      _ = try await runtimeClient.stop()
      await identityStore.lock()
      try qualificationEvidenceStore?.cleanup()
      phase = .stopped
    } catch let TeraRuntimeClientError.shutdown(failure) {
      phase = .failed(failure)
    } catch {
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
    let signer = try await identityStore.signer(for: identity)
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
      _ = try? await runtimeClient.stop()
      throw TeraRuntimeClientError.superseded
    }
    try await reconcileIdentity(identity)
    if configuration.activationState == .reconfigurationRequired,
      adoptBootstrapSettings
    {
      try await configurationStore.confirmBootstrapActivation(
        expectedGeneration: configuration.generation
      )
    }
    guard generation == requestedGeneration else {
      _ = try? await runtimeClient.stop()
      throw TeraRuntimeClientError.superseded
    }
    return .running(snapshot)
  }

  private func reconcileIdentity(_ identity: TeraAppIdentity) async throws {
    guard let identityID = identity.identityHandle,
      let publicKeyHex = identity.publicKeyHex
    else {
      throw TeraIdentityStoreError.unavailable
    }
    var settings = try await runtimeClient.mobileSettings()
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
    }
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
      settings = try await runtimeClient.applyIdentityCommand(
        expectedRevision: settings.revision,
        command: TeraIdentityCommand(
          kind: .completeImport,
          operationID: operationID,
          identityID: identityID,
          publicKeyHex: publicKeyHex
        )
      ).settings
    }
    _ = try await runtimeClient.applyIdentityCommand(
      expectedRevision: settings.revision,
      command: TeraIdentityCommand(
        kind: .unlock,
        operationID: nil,
        identityID: nil,
        publicKeyHex: nil
      )
    )
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

  @MainActor
  private static func requiredString(_ key: String, bundle: Bundle) throws -> String {
    guard let value = normalizedOptional(bundle.object(forInfoDictionaryKey: key) as? String) else {
      throw TeraConfigurationError.missing(key)
    }
    return value
  }

  @MainActor
  private static func array(_ key: String, bundle: Bundle) -> [String] {
    if let values = bundle.object(forInfoDictionaryKey: key) as? [String] {
      return values.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
    }
    guard let raw = normalizedOptional(bundle.object(forInfoDictionaryKey: key) as? String) else {
      return []
    }
    return raw.components(separatedBy: CharacterSet(charactersIn: ",; \n\r\t"))
      .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
      .filter { !$0.isEmpty }
  }

  @MainActor
  private static func normalizedOptional(_ value: String?) -> String? {
    guard let value else { return nil }
    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty || trimmed == "unknown" ? nil : trimmed
  }
}
