import Foundation
import RadrootsKit

@MainActor
struct RadrootsProductStores {
  let today: RadrootsTodayStore
  let add: RadrootsAddStore
  let search: RadrootsSearchStore
  let me: RadrootsMeStore
  let settings: RadrootsSettingsStore
  let media: RadrootsMediaStore

  init(
    runtimeClient: RadrootsRuntimeClient,
    addMedia: (any RadrootsAddMediaHandling)? = nil
  ) {
    today = RadrootsTodayStore(runtimeClient: runtimeClient)
    add = RadrootsAddStore(runtimeClient: runtimeClient, media: addMedia)
    search = RadrootsSearchStore(runtimeClient: runtimeClient)
    me = RadrootsMeStore(runtimeClient: runtimeClient)
    settings = RadrootsSettingsStore(runtimeClient: runtimeClient)
    media = RadrootsMediaStore(runtimeClient: runtimeClient)
  }

  func configure(snapshot: RadrootsRuntimeSnapshot) {
    today.configure(snapshot: snapshot)
    add.configure(snapshot: snapshot)
  }

  func resume() async {
    await today.start()
    await add.start()
  }

  func suspend() {
    today.stop()
    add.suspend()
    search.stop()
    me.stop()
    settings.stop()
    media.reset()
  }

  func stop() {
    today.stop()
    add.stop()
    search.stop()
    me.stop()
    settings.stop()
    media.reset()
  }
}

@MainActor
final class RadrootsAppModel: ObservableObject {
  typealias Phase = RadrootsSessionPhase

  @Published private(set) var phase: Phase = .starting
  let productStores: RadrootsProductStores?
  let diagnosticsStore: RadrootsDiagnosticsStore

  private let sessionStore: RadrootsSessionStore?
  private let bootstrapFailure: RadrootsRuntimeFailure?
  private let lifecycleCoordinator: RadrootsLifecycleCoordinator
  private var generation: UInt64 = 0
  private var lifecycleRegistered = false
  private var sessionOperationsInFlight = 0
  private var resumePending = false
  private let isShellUITest: Bool

  init(
    sessionStore: RadrootsSessionStore? = nil,
    runtimeClient: RadrootsRuntimeClient? = nil,
    lifecycleCoordinator requestedLifecycleCoordinator: RadrootsLifecycleCoordinator? = nil
  ) {
    let productionServices = Bundle.main.bundleIdentifier.flatMap {
      try? RadrootsLifecycleCoordinator.productionServices(bundleIdentifier: $0)
    }
    let lifecycleCoordinator =
      requestedLifecycleCoordinator
      ?? productionServices?.coordinator
      ?? RadrootsLifecycleCoordinator.disabled()
    let backgroundTransfer =
      requestedLifecycleCoordinator == nil
      ? productionServices?.backgroundTransfer : nil
    self.lifecycleCoordinator = lifecycleCoordinator
    diagnosticsStore = RadrootsDiagnosticsStore(coordinator: lifecycleCoordinator)
    #if DEBUG
      if ProcessInfo.processInfo.environment["RADROOTS_IOS_UI_TEST_SHELL"] == "1" {
        self.sessionStore = nil
        productStores = nil
        bootstrapFailure = nil
        isShellUITest = true
        phase = .running(.shellUITest)
        return
      }
    #endif
    isShellUITest = false
    if let sessionStore {
      self.sessionStore = sessionStore
      productStores = runtimeClient.map { RadrootsProductStores(runtimeClient: $0) }
      bootstrapFailure = nil
    } else {
      do {
        let runtimeClient = runtimeClient ?? .production()
        let sessionStore = try RadrootsSessionStore.production(
          runtimeClient: runtimeClient
        )
        let mediaCoordinator = try Self.productionMediaCoordinator(
          transfer: backgroundTransfer
        )
        self.sessionStore = sessionStore
        productStores = RadrootsProductStores(
          runtimeClient: runtimeClient,
          addMedia: mediaCoordinator
        )
        bootstrapFailure = nil
      } catch let error as LocalizedError {
        self.sessionStore = nil
        productStores = nil
        bootstrapFailure = .local(
          operation: "app.bootstrap",
          code: "ios.app.configuration_invalid",
          safeMessage: error.errorDescription ?? "Radroots configuration is invalid."
        )
      } catch {
        self.sessionStore = nil
        productStores = nil
        bootstrapFailure = .local(
          operation: "app.bootstrap",
          code: "ios.app.configuration_invalid",
          safeMessage: "Radroots configuration is invalid."
        )
      }
    }
  }

  func start() async {
    await ensureLifecycleRegistration()
    await lifecycleCoordinator.record("ios.lifecycle.start_requested")
    await run(name: "start", showsStarting: true) { store in await store.start() }
  }

  func retry() async {
    await start()
  }

  func createIdentity() async {
    await run(name: "identity_create", showsStarting: true) { store in
      await store.createIdentity()
    }
  }

  func importIdentity(_ material: RadrootsIdentitySecretMaterial) async {
    await run(name: "identity_import", showsStarting: true) { store in
      await store.importIdentity(material)
    }
  }

  func lockIdentity() async {
    stopPresentationWork()
    await run(name: "identity_lock") { store in await store.lockIdentity() }
  }

  func unlockIdentity() async {
    await run(name: "identity_unlock", showsStarting: true) { store in
      await store.unlockIdentity()
    }
  }

  func recoverIdentity() async {
    await run(name: "identity_recover", showsStarting: true) { store in
      await store.recoverIdentity()
    }
  }

  func applyConfigurationReconfiguration() async {
    await run(name: "configuration_reconfigure", showsStarting: true) { store in
      await store.applyConfigurationReconfiguration()
    }
  }

  func applySettingsReconfiguration() async {
    stopPresentationWork()
    await run(name: "settings_reconfigure", showsStarting: true) { store in
      await store.applySettingsReconfiguration()
    }
  }

  func stop() async {
    stopPresentationWork()
    await run(name: "stop") { store in await store.stop() }
  }

  func resume() async {
    await ensureLifecycleRegistration()
    guard sessionOperationsInFlight == 0 else {
      resumePending = true
      await lifecycleCoordinator.record("ios.lifecycle.resume_coalesced")
      return
    }
    resumePending = false
    if case .running = phase {
      await productStores?.resume()
      await lifecycleCoordinator.record("ios.lifecycle.active")
    } else {
      await start()
    }
  }

  func suspend() async {
    generation &+= 1
    resumePending = false
    productStores?.suspend()
    await sessionStore?.suspend()
    await lifecycleCoordinator.record("ios.lifecycle.background")
  }

  func updateProtectedDataAvailability(_ available: Bool) async {
    await sessionStore?.updateProtectedDataAvailability(available)
    await lifecycleCoordinator.record(
      available
        ? "ios.lifecycle.protected_data_available"
        : "ios.lifecycle.protected_data_unavailable"
    )
    if available {
      await start()
    } else {
      await stop()
      await start()
    }
  }

  func shutdown() async {
    await lifecycleCoordinator.record("ios.lifecycle.shutdown_requested", level: .notice)
    await stop()
    await RadrootsBackgroundEventRouter.shared.detachAndCompletePending()
  }

  private func run(
    name: String,
    showsStarting: Bool = false,
    _ operation: @escaping @Sendable (RadrootsSessionStore) async -> Phase
  ) async {
    guard !isShellUITest else { return }
    sessionOperationsInFlight += 1
    generation &+= 1
    let requestedGeneration = generation
    if showsStarting {
      phase = .starting
    }
    guard let sessionStore else {
      phase = .failed(
        bootstrapFailure
          ?? .local(
            operation: "app.bootstrap",
            code: "ios.app.bootstrap_failed",
            safeMessage: "Radroots could not start."
          )
      )
      await lifecycleCoordinator.record(
        "ios.lifecycle.operation_failed",
        level: .error,
        fields: ["operation": name, "code": "bootstrap_failed"]
      )
      await finishSessionOperation()
      return
    }
    let result = await operation(sessionStore)
    guard generation == requestedGeneration else {
      await finishSessionOperation()
      return
    }
    if case .running(let snapshot) = result {
      productStores?.configure(snapshot: snapshot)
    }
    phase = result
    await lifecycleCoordinator.record(
      "ios.lifecycle.operation_completed",
      level: Self.isFailure(result) ? .warning : .info,
      fields: ["operation": name, "phase": Self.phaseCode(result)]
    )
    await finishSessionOperation()
  }

  private func finishSessionOperation() async {
    sessionOperationsInFlight -= 1
    guard sessionOperationsInFlight == 0, resumePending else { return }
    resumePending = false
    await resume()
  }

  private func ensureLifecycleRegistration() async {
    guard !lifecycleRegistered else { return }
    lifecycleRegistered = true
    await lifecycleCoordinator.attachBackgroundEvents()
    await RadrootsLifecycleBridge.shared.register { @Sendable [weak self] in
      await self?.shutdown()
    }
  }

  private func stopPresentationWork() {
    productStores?.stop()
  }

  private static func isFailure(_ phase: Phase) -> Bool {
    if case .failed = phase {
      return true
    }
    return false
  }

  private static func phaseCode(_ phase: Phase) -> String {
    switch phase {
    case .starting: "starting"
    case .identityRequired: "identity_required"
    case .identityLocked: "identity_locked"
    case .protectedDataUnavailable: "protected_data_unavailable"
    case .recoveryRequired: "recovery_required"
    case .corruptIdentity: "corrupt_identity"
    case .configurationReconfigurationRequired: "configuration_reconfiguration_required"
    case .running: "running"
    case .stopped: "stopped"
    case .failed: "failed"
    }
  }

  private static func productionMediaCoordinator(
    bundle: Bundle = .main,
    transfer: (any RadrootsBackgroundTransfer)?
  ) throws -> RadrootsAddMediaCoordinator {
    guard let bundleIdentifier = bundle.bundleIdentifier else {
      throw RadrootsConfigurationError.missing("bundle_identifier")
    }
    guard let transfer else {
      throw RadrootsConfigurationError.missing("background_transfer")
    }
    return try RadrootsAddMediaCoordinator.production(
      bundleIdentifier: bundleIdentifier,
      transfer: transfer
    )
  }
}

#if DEBUG
  extension RadrootsRuntimeSnapshot {
    fileprivate static let shellUITest = RadrootsRuntimeSnapshot(
      identity: RadrootsRuntimeIdentity(
        publicKeyHex: String(repeating: "ab", count: 32),
        hostSignerConfigured: true
      ),
      relay: RadrootsRelayStatus(
        profile: "simulator",
        state: "configured",
        readAvailability: "unobserved",
        writeAvailability: "unobserved",
        relays: []
      ),
      blossomConfiguration: nil,
      blossomEvidence: nil,
      crateName: "radroots_mobile_ffi",
      crateVersion: "0.1.0-alpha",
      isClosed: false
    )
  }
#endif
