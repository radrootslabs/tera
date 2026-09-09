import Foundation
@testable import TeraApp

actor ResourceTestGate {
  private var opened = false
  private var waiters: [CheckedContinuation<Void, Never>] = []

  func wait() async {
    guard !opened else { return }
    await withCheckedContinuation { waiters.append($0) }
  }

  func open() {
    guard !opened else { return }
    opened = true
    let pending = waiters
    waiters.removeAll()
    for waiter in pending {
      waiter.resume()
    }
  }
}

struct ResourceTestPause: Sendable {
  let entered = ResourceTestGate()
  let resume = ResourceTestGate()

  func wait() async {
    await entered.open()
    await resume.wait()
  }
}

actor ResourceTestToken: TeraRuntimeSubscriptionToken {
  let cancelled = ResourceTestGate()
  private(set) var cancelCount = 0

  func cancel() async {
    cancelCount += 1
    await cancelled.open()
  }
}

actor ResourceTestBackend: TeraRuntimeBackend {
  let token = ResourceTestToken()
  let closed = ResourceTestGate()
  private(set) var shutdownCount = 0
  private let value: TeraRuntimeSnapshot
  private var subscriptionPause: ResourceTestPause?
  private var snapshotPause: ResourceTestPause?
  private var settingsPause: ResourceTestPause?
  private var profilePause: ResourceTestPause?
  private var shutdownPause: ResourceTestPause?
  private var shutdownFailure: TeraRuntimeFailure?
  private var shutdownCompleted = false
  private(set) var profileMutations = 0
  private var receive: (@Sendable (TeraRuntimeChange) async -> Void)?

  init(publicKeyHex: String) {
    value = TeraRuntimeSnapshot(
      identity: TeraRuntimeIdentity(publicKeyHex: publicKeyHex, hostSignerConfigured: true),
      relay: nil, blossomConfiguration: nil, blossomEvidence: nil,
      crateName: "tera_ffi", crateVersion: "0.1.0-alpha", isClosed: false
    )
  }

  func start() -> TeraRuntimeBackendStart {
    TeraRuntimeBackendStart(backend: self, snapshot: value)
  }

  func pauseSubscription(_ pause: ResourceTestPause) {
    subscriptionPause = pause
  }

  func pauseSnapshot(_ pause: ResourceTestPause) {
    snapshotPause = pause
  }

  func pauseSettings(_ pause: ResourceTestPause) {
    settingsPause = pause
  }

  func pauseShutdown(_ pause: ResourceTestPause) {
    shutdownPause = pause
  }

  func failShutdownOnce(_ failure: TeraRuntimeFailure) {
    shutdownFailure = failure
  }

  func pauseProfile(_ pause: ResourceTestPause) {
    profilePause = pause
  }

  func saveProfileMetadata(input _: TeraProfileMetadataInput) async -> TeraProfileStatus {
    await profileMutation()
  }

  func advanceProfile(operationID _: String) async -> TeraProfileStatus {
    await profileMutation()
  }

  func cancelProfile(operationID _: String, expectedRevision _: UInt64) async -> TeraProfileStatus {
    await profileMutation()
  }

  private func profileMutation() async -> TeraProfileStatus {
    profileMutations += 1
    let pause = profilePause
    profilePause = nil
    await pause?.wait()
    return TeraProfileStatus(
      id: String(repeating: "a", count: 32), revision: UInt64(profileMutations),
      authorPublicKey: value.identity.publicKeyHex, state: .draft, deliveryID: nil,
      createdAtUnixMilliseconds: 1_800_000_000_000, updatedAtUnixMilliseconds: 1_800_000_000_000,
      settlement: nil
    )
  }

  func snapshot() async throws -> TeraRuntimeSnapshot {
    let pause = snapshotPause
    snapshotPause = nil
    await pause?.wait()
    guard shutdownCount == 0 else { throw unsupported() }
    return value
  }

  func todayPage(request _: TeraTodayPageRequest) throws -> TeraTodayPage {
    throw unsupported()
  }

  func refreshToday(
    context _: TeraLocalNetwork, nowUnixSeconds _: UInt64, update _: TeraTodayProjectionUpdate
  ) throws -> TeraTodayRefreshReceipt {
    throw unsupported()
  }

  func subscribe(
    bufferCapacity _: Int, receive: @escaping @Sendable (TeraRuntimeChange) async -> Void
  ) async -> any TeraRuntimeSubscriptionToken {
    self.receive = receive
    let pause = subscriptionPause
    subscriptionPause = nil
    await pause?.wait()
    return token
  }

  func emit(_ revision: UInt64) async {
    await receive?(TeraRuntimeChange(
      schemaVersion: 1, generation: TeraProjectionRevision(rawValue: revision), kind: .today, entityID: nil
    ))
  }

  func shutdown() async throws -> TeraRuntimeShutdownReceipt {
    shutdownCount += 1
    let pause = shutdownPause
    shutdownPause = nil
    await pause?.wait()
    if let failure = shutdownFailure {
      shutdownFailure = nil
      throw failure
    }
    let alreadyClosed = shutdownCompleted
    shutdownCompleted = true
    await closed.open()
    return TeraRuntimeShutdownReceipt(state: "closed", alreadyClosed: alreadyClosed)
  }

  func mobileSettings() async -> TeraMobileSettings {
    let pause = settingsPause
    settingsPause = nil
    await pause?.wait()
    return settings()
  }

  func applyIdentityCommand(
    expectedRevision _: UInt64, command _: TeraIdentityCommand
  ) -> TeraSettingsTransition {
    TeraSettingsTransition(
      settings: settings(), runtimeRestartRequired: false,
      outboxRequeueRequired: false, mediaCacheInvalidationRequired: false
    )
  }

  private func settings() -> TeraMobileSettings {
    TeraMobileSettings(
      revision: 1,
      identity: TeraSettingsIdentityState(
        identities: [TeraSettingsIdentity(id: "identity", publicKeyHex: value.identity.publicKeyHex)],
        activeIdentityID: "identity", lockState: .unlocked, pendingImportOperationID: nil
      ),
      networkEnvironment: .simulator,
      relays: [TeraRelayPreference(url: "ws://127.0.0.1:7447", access: .readWrite)],
      blossomAuthority: .loopbackDevelopment, blossomPrimaryOrigin: "http://127.0.0.1:3000",
      blossomFallbackOrigins: [], allowCellularDownloads: true,
      allowCellularUploads: true, allowBackgroundTransfers: true,
      mediaCacheBytes: 256 * 1_048_576, mediaCacheArtifacts: 1024
    )
  }

  private func unsupported() -> TeraRuntimeFailure {
    .local(operation: "test.resource", code: "test.unavailable", safeMessage: "Test operation unavailable")
  }
}
