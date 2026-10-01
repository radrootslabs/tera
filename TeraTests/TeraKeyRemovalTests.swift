import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraKeyRemovalTests: XCTestCase {
  func testSuccessfulRemovalRetainsLocalDataAndPreparedRequests() async throws {
    let fixture = try await RemovalFixture()
    defer { fixture.state.remove() }
    let result = await fixture.remove([.init(id: "first", revision: 1)])
    XCTAssertEqual(result, .identityRequired)
    let identity = await fixture.identity.snapshot()
    let prepared = await fixture.backend.preparedRemovals
    let closes = await fixture.backend.shutdownCount
    XCTAssertEqual(identity.state, .absent)
    XCTAssertEqual(prepared, ["first"])
    XCTAssertEqual(closes, 1)
    XCTAssertEqual(try Data(contentsOf: fixture.retained), Data("retained local data".utf8))
    let restarted = await fixture.session.start()
    XCTAssertEqual(restarted, .identityRequired, "No silent identity regeneration")
  }

  func testPreparationFailurePreservesKeyAndEarlierSignedRequests() async throws {
    let fixture = try await RemovalFixture()
    defer { fixture.state.remove() }
    await fixture.backend.configureRemoval(failureID: "second")
    _ = await fixture.remove([.init(id: "first", revision: 1), .init(id: "second", revision: 1)])
    let prepared = await fixture.backend.preparedRemovals
    let closes = await fixture.backend.shutdownCount
    XCTAssertEqual(prepared, ["first"])
    XCTAssertEqual(closes, 0)
    await assertRetained(fixture)
  }

  func testCancellationBeforeAndDuringPreparationPreservesKey() async throws {
    for during in [false, true] {
      let fixture = try await RemovalFixture()
      defer { fixture.state.remove() }
      let pause = ResourceTestPause()
      if during {
        await fixture.backend.configureRemoval(pause: pause)
      }
      let task = Task {
        if !during {
          await pause.wait()
        }
        return await fixture.remove([.init(id: "first", revision: 1)])
      }
      await pause.entered.wait()
      task.cancel()
      await pause.resume.open()
      _ = await task.value
      await assertRetained(fixture)
    }
  }

  func testCancellationAfterPreparationAndShutdownFailurePreserveKey() async throws {
    for failClose in [false, true] {
      let fixture = try await RemovalFixture()
      defer { fixture.state.remove() }
      let pause = ResourceTestPause()
      await fixture.backend.pauseShutdown(pause)
      if failClose {
        await fixture.backend.failShutdownOnce(.local(operation: "test.close", code: "test.close", safeMessage: "Close failed"))
      }
      let task = Task { await fixture.remove([.init(id: "first", revision: 1)]) }
      await pause.entered.wait()
      if !failClose {
        task.cancel()
      }
      await pause.resume.open()
      _ = await task.value
      let prepared = await fixture.backend.preparedRemovals
      XCTAssertEqual(prepared, ["first"])
      await assertRetained(fixture)
    }
  }

  func testCancellationAtPresenceAndStaleAuthorCannotDeleteKey() async throws {
    let fixture = try await RemovalFixture()
    defer { fixture.state.remove() }
    _ = await fixture.session.removeSigningKey(author: String(repeating: "ab", count: 32), requests: [])
    await assertRetained(fixture)
    let pause = ResourceTestPause()
    await fixture.presence.arm(pause)
    let task = Task { await fixture.remove([]) }
    await pause.entered.wait()
    // A concurrent start cannot replace/reconfigure the identity during removal.
    _ = await fixture.session.start()
    task.cancel()
    await pause.resume.open()
    _ = await task.value
    await assertRetained(fixture)
  }

  func testLateCancellationDoesNotHideCompletedDeletion() async throws {
    let fixture = try await RemovalFixture()
    defer { fixture.state.remove() }
    fixture.secure.cancelOnRemoval()
    let task = Task { await fixture.remove([]) }
    let result = await task.value
    XCTAssertEqual(result, .identityRequired)
    let observed = await fixture.identity.snapshot()
    XCTAssertEqual(observed.state, .absent)
    XCTAssertTrue(FileManager.default.fileExists(atPath: fixture.retained.path))
  }

  func testDeniedRemovalPreservesIdentityAndRetainedData() async throws {
    let fixture = try await RemovalFixture()
    defer { fixture.state.remove() }
    await fixture.presence.deny()
    _ = await fixture.remove([])
    await assertRetained(fixture)
  }

  @MainActor
  func testAppModelReportsCompletedRemovalAfterLateCancellation() async throws {
    let fixture = try await RemovalFixture()
    defer { fixture.state.remove() }
    let model = TeraAppModel(sessionStore: fixture.session, lifecycleCoordinator: .disabled())
    await model.start()
    fixture.secure.cancelOnRemoval()
    let task = Task { await model.removeSigningKey(author: fixture.author, requests: []) }
    await task.value
    XCTAssertEqual(model.phase, .identityRequired)
  }

  func testConsentSeparatesRetainedDataRemoteCopiesAndRecoveryRequirement() {
    XCTAssertTrue(TeraKeyRemovalConsent.local.contains("does not erase local data"))
    XCTAssertTrue(TeraKeyRemovalConsent.remote.contains("Published copies may remain"))
    XCTAssertTrue(TeraKeyRemovalConsent.recovery.contains("import the same key"))
    XCTAssertTrue(TeraKeyRemovalConsent.recovery.contains("delivery is not guaranteed"))
  }

  private func assertRetained(_ fixture: RemovalFixture) async {
    let identity = await fixture.identity.snapshot()
    XCTAssertNotEqual(identity.state, .absent)
    XCTAssertEqual(identity.publicKeyHex, fixture.author)
    XCTAssertTrue(FileManager.default.fileExists(atPath: fixture.retained.path))
  }
}

private struct RemovalFixture: Sendable {
  let state: StateFixture
  let secure: RemovalSecureStore
  let presence: RemovalPresence
  let identity: TeraIdentityStore
  let backend: ResourceTestBackend
  let session: TeraSessionStore
  let author: String
  let retained: URL

  init() async throws {
    let state = try StateFixture()
    self.state = state
    retained = state.root.appendingPathComponent("retained-data")
    try Data("retained local data".utf8).write(to: retained)
    let secure = RemovalSecureStore()
    self.secure = secure
    let presence = RemovalPresence()
    self.presence = presence
    let custody = try RadrootsIdentityCustody(
      configuration: RadrootsIdentityCustodyConfiguration(namespace: "radroots_identity_v1", secretPolicy: .secureLocalSecret),
      secureStore: secure, metadataStore: InMemoryIdentityMetadataStore(), userPresence: presence
    )
    let identity = TeraIdentityStore(custody: custody, secureStore: secure, servicePrefix: UUID().uuidString)
    self.identity = identity
    let created = try await identity.create()
    let author = try XCTUnwrap(created.publicKeyHex)
    self.author = author
    let backend = ResourceTestBackend(publicKeyHex: author)
    self.backend = backend
    let client = TeraRuntimeClient(factory: { _ in await backend.start() })
    session = TeraSessionStore(configurationStore: TeraConfigurationStore(bootstrap: state.bootstrap, roots: state.roots),
                               identityStore: identity, runtimeClient: client, roots: state.roots,
                               protectedData: TeraProtectedDataMonitor(available: true))
    guard case .running = await session.start() else { throw TeraIdentityStoreError.unavailable }
  }

  func remove(_ requests: [TeraKeyRemovalRequest]) async -> TeraSessionPhase {
    await session.removeSigningKey(author: author, requests: requests)
  }
}

private actor RemovalPresence: RadrootsUserPresence {
  private var pause: ResourceTestPause?
  private var allowed = true
  func deny() {
    allowed = false
  }

  func arm(_ pause: ResourceTestPause) {
    self.pause = pause
  }

  func currentStatus() async throws -> RadrootsUserPresenceStatus {
    .unavailable
  }

  func verify(_ request: RadrootsUserPresenceRequest) async throws -> RadrootsUserPresenceResult {
    let pending = pause
    pause = nil
    await pending?.wait()
    return RadrootsUserPresenceResult(policy: request.policy, verified: allowed)
  }
}

private final class RemovalSecureStore: RadrootsSecureStore, @unchecked Sendable {
  private let storage = InMemorySecureStore()
  private let lock = NSLock()
  private var cancels = false
  func cancelOnRemoval() {
    lock.withLock { cancels = true }
  }

  func put(_ value: Data, for key: RadrootsSecureStoreKey, policy: RadrootsSecretAccessPolicy) throws {
    try storage.put(value, for: key, policy: policy)
  }

  func get(_ key: RadrootsSecureStoreKey) throws -> Data? {
    try storage.get(key)
  }

  func contains(_ key: RadrootsSecureStoreKey) throws -> Bool {
    try storage.contains(key)
  }

  func delete(_ key: RadrootsSecureStoreKey) throws {
    try storage.delete(key)
    if key.name == "active_secret_v1", lock.withLock({ cancels }) {
      withUnsafeCurrentTask { $0?.cancel() }
    }
  }

  func deleteNamespace(_ namespace: String) throws {
    try storage.deleteNamespace(namespace)
  }
}
