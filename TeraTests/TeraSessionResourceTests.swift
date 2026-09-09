import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraSessionResourceTests: XCTestCase {
  func testStaleStartupSnapshotDoesNotStopNewSharedSession() async throws {
    let fixture = try await Fixture()
    defer { fixture.state.remove() }
    let initial = await fixture.session.start()
    guard case .running = initial else { return XCTFail("Initial session must run") }
    let pause = ResourceTestPause()
    await fixture.backend.pauseSnapshot(pause)
    let old = Task { await fixture.session.start() }
    await pause.entered.wait()
    let current = await fixture.session.start()
    guard case .running = current else { return XCTFail("New session must adopt the shared runtime") }
    let commands = await fixture.backend.identityCommands
    await pause.resume.open()
    _ = await old.value
    try await assertRunning(fixture, phase: current)
    let after = await fixture.backend.identityCommands
    XCTAssertEqual(after, commands, "Superseded reconciliation cannot issue identity commands in the live session")
    _ = await fixture.session.stop()
  }

  func testStaleIdentityReconciliationDoesNotStopNewSharedSession() async throws {
    let fixture = try await Fixture()
    defer { fixture.state.remove() }
    let pause = ResourceTestPause()
    await fixture.backend.pauseSettings(pause)
    let old = Task { await fixture.session.start() }
    await pause.entered.wait()
    let current = await fixture.session.start()
    guard case .running = current else { return XCTFail("New session must adopt the shared runtime") }
    let commands = await fixture.backend.identityCommands
    await pause.resume.open()
    _ = await old.value
    try await assertRunning(fixture, phase: current)
    let after = await fixture.backend.identityCommands
    XCTAssertEqual(after, commands, "Superseded reconciliation cannot issue identity commands in the live session")
    _ = await fixture.session.stop()
  }

  private func assertRunning(_ fixture: Fixture, phase: TeraSessionPhase) async throws {
    let observed = await fixture.session.currentPhase()
    let closes = await fixture.backend.shutdownCount
    let snapshot = try await fixture.client.snapshot()
    XCTAssertEqual(observed, phase)
    XCTAssertFalse(snapshot.isClosed)
    XCTAssertEqual(closes, 0)
  }

  private struct Fixture {
    let state: StateFixture
    let backend: ResourceTestBackend
    let client: TeraRuntimeClient
    let session: TeraSessionStore

    init() async throws {
      state = try StateFixture()
      let secure = InMemorySecureStore()
      let custody = try RadrootsIdentityCustody(
        configuration: RadrootsIdentityCustodyConfiguration(
          namespace: "radroots_identity_v1", secretPolicy: .secureLocalSecret
        ),
        secureStore: secure, metadataStore: InMemoryIdentityMetadataStore(),
        userPresence: AllowingUserPresence()
      )
      let identity = TeraIdentityStore(
        custody: custody, secureStore: secure,
        servicePrefix: "org.tera.tests.session.\(UUID().uuidString.lowercased())"
      )
      let created = try await identity.create()
      let backend = try ResourceTestBackend(publicKeyHex: XCTUnwrap(created.publicKeyHex))
      self.backend = backend
      let client = TeraRuntimeClient(factory: { _ in await backend.start() })
      self.client = client
      session = TeraSessionStore(
        configurationStore: TeraConfigurationStore(bootstrap: state.bootstrap, roots: state.roots),
        identityStore: identity, runtimeClient: client, roots: state.roots,
        protectedData: TeraProtectedDataMonitor(available: true)
      )
    }
  }
}
