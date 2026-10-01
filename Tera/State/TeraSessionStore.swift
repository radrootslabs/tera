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
  let configurationStore: TeraConfigurationStore
  let identityStore: TeraIdentityStore
  let runtimeClient: TeraRuntimeClient
  let roots: RadrootsAppleFileRoots
  let protectedData: TeraProtectedDataMonitor
  private let automatesQualificationIdentity: Bool
  let qualificationEvidenceStore: TeraRemoteQualificationEvidenceStore?
  var generation = TeraSessionGeneration.initial
  var phase: TeraSessionPhase = .starting
  var removalInProgress = false

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
    guard !removalInProgress else { return phase }
    return await start(acceptingReconfiguration: false)
  }

  func applyConfigurationReconfiguration() async -> TeraSessionPhase {
    guard !removalInProgress else { return phase }
    return await start(acceptingReconfiguration: true)
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
    guard !removalInProgress else { return phase }
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

  func isCurrent(_ requested: TeraSessionGeneration) -> Bool {
    generation == requested && generation.isActive && !Task.isCancelled
  }

  func ensureCurrent(_ requested: TeraSessionGeneration) throws {
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
