import Foundation
import RadrootsKit
@testable import TeraApp
import TeraKitBindings
import XCTest

@MainActor
final class TeraLateSigningTests: XCTestCase {
  func testDurableStopCrossesGeneratedBridgeWhileSignerOwnsAdmission() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let signer = try await LateNativeSigner.make()
    let client = client(clock: LateSigningClock())
    let configuration = configuration(fixture, signer: signer)
    _ = try await client.start(configuration: configuration)
    var form = TeraAddForm.empty(.createUpdate)
    form.content = "Original stopped native capture"
    let source = try await client.saveComposer(request: TeraComposerSaveRequest(
      scope: scope, id: client.reserveComposerID(), expectedRevision: nil, editSequence: 1, form: TeraComposerForm(editing: form)
    ))
    let request = try await TeraSubmissionRequest(commandID: client.reserveSubmissionID(), scope: scope,
                                                  composerID: source.draft.id, expectedRevision: 1)
    let prepared = try await client.prepareSubmission(request: request, media: [])
    let waiting = Task { try? await client.advanceSubmission(request: request, expectedRevision: prepared.revision) }
    await signer.pause.entered.wait()
    let stopped = try await client.requestSubmissionStop(request: request)
    XCTAssertTrue(stopped.delivery.isStopped)
    XCTAssertEqual(stopped.delivery.state, .notIssued)
    let replay = try await client.requestSubmissionStop(request: request)
    XCTAssertEqual(replay, stopped)
    let whileHeld = try await client.submissionStatus(request: request)
    XCTAssertEqual(whileHeld, stopped)
    await signer.pause.resume.open()
    _ = await waiting.value
    let retained = try await signedStatus(client, request: request)
    XCTAssertEqual(retained.delivery, stopped.delivery)
    XCTAssertEqual(retained.settlement.signed, 1)
    XCTAssertEqual(retained.settlement.admitted, 0)
    XCTAssertEqual(retained.state, .cancelled)
    let context = TeraLocalNetwork(schemaVersion: 1, id: scope.localNetworkID, label: "Nearby",
                                   relayURLs: ["ws://127.0.0.1:19999"], locality: nil, followedAuthors: [], generation: 1)
    let local = try await client.reconcileSubmissionLocal(request: request, context: context)
    XCTAssertEqual(local, retained)
    XCTAssertEqual(local.settlement.admitted, 0)
    _ = try await client.stop()
    await signer.disable()
    _ = try await client.start(configuration: configuration)
    let reopened = try await client.submissionStatus(request: request)
    XCTAssertEqual(reopened, retained)
    do {
      _ = try await client.advanceSubmission(request: request, expectedRevision: retained.revision)
      XCTFail("A retained stop cannot authorize another effect")
    } catch { XCTAssertEqual(TeraAddPresentation.failure(for: error)?.code, "submission_stopped") }
    let count = await signer.count()
    XCTAssertEqual(count, 1)
    _ = try await client.stop()
  }

  func testCancelledNativeWaitRetainsLateSignatureAcrossRestart() async throws {
    try await checkLateSigning(cancelWait: true, failHostClock: false)
  }

  func testNativeDeadlineAndMissingHostClockRetainLateSignatureAcrossRestart() async throws {
    try await checkLateSigning(cancelWait: false, failHostClock: true)
  }

  private func checkLateSigning(cancelWait: Bool, failHostClock: Bool) async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let signer = try await LateNativeSigner.make()
    let clock = LateSigningClock()
    let client = client(clock: clock)
    let configuration = configuration(fixture, signer: signer)
    _ = try await client.start(configuration: configuration)
    var form = TeraAddForm.empty(.createUpdate)
    form.content = "PRIVATE original native signing capture"
    let source = try await client.saveComposer(request: TeraComposerSaveRequest(
      scope: scope, id: client.reserveComposerID(), expectedRevision: nil, editSequence: 1,
      form: TeraComposerForm(editing: form)
    ))
    let request = try await TeraSubmissionRequest(commandID: client.reserveSubmissionID(), scope: scope,
                                                  composerID: source.draft.id, expectedRevision: 1)
    let prepared = try await client.prepareSubmission(request: request, media: [])
    let waiting = Task {
      do {
        _ = try await client.advanceSubmission(request: request, expectedRevision: prepared.revision)
        return true
      } catch { return false }
    }
    await signer.pause.entered.wait()
    if cancelWait {
      waiting.cancel()
    }
    let returnedSuccess = await waiting.value
    XCTAssertFalse(returnedSuccess)
    let pending = try await client.submissionStatus(request: request)
    XCTAssertEqual(pending.captured, prepared.captured)
    XCTAssertEqual(pending.settlement.signed, 0)
    do {
      _ = try await client.advanceSubmission(request: request, expectedRevision: pending.revision)
      XCTFail("The original native callback still owns admission")
    } catch {
      XCTAssertEqual(TeraAddPresentation.failure(for: error)?.code, "operation_in_progress")
    }
    try await passSigningDeadline(signer)
    if failHostClock {
      clock.fail()
    }
    await signer.pause.resume.open()
    let retained = try await signedStatus(client, request: request)
    XCTAssertEqual(retained.captured, prepared.captured)
    XCTAssertEqual(retained.operationID, prepared.operationID)
    XCTAssertEqual(retained.settlement.signed, 1)
    XCTAssertEqual(retained.settlement.admitted, 0)
    XCTAssertEqual(retained.settlement.deliverySatisfied, 0)
    try await checkReconstruction(client, signer: signer, configuration: configuration, request: request, retained: retained)
  }

  private func passSigningDeadline(_ signer: LateNativeSigner) async throws {
    let capturedDeadline = await signer.deadline()
    let deadline = try XCTUnwrap(capturedDeadline)
    let now = try TeraClock.system.unixMilliseconds(requirePositive: true)
    let remaining = deadline > now ? deadline - now : 0
    XCTAssertLessThanOrEqual(remaining, 30000)
    try await Task.sleep(nanoseconds: (remaining + 20) * 1_000_000)
  }

  private func client(clock: LateSigningClock) -> TeraRuntimeClient {
    TeraRuntimeClient(factory: { configuration in
      let runtime = try await TeraRuntime.withHostSigner(
        applicationSupportDirectory: configuration.applicationSupportDirectory,
        publicKeyHex: configuration.publicKeyHex,
        sourceGenerationHex: configuration.sourceGenerationHex,
        sourceGenerationCreatedAtUnixMs: configuration.sourceGenerationCreatedAtUnixMilliseconds,
        protectedData: .available,
        hostSigner: TeraGeneratedHostSigner(signer: configuration.signer, clock: TeraClock(now: { clock.now() }))
      )
      try runtime.configureSimulatorRelays(loopbackRelays: ["ws://127.0.0.1:19999"])
      let backend = TeraGeneratedRuntimeBackend(runtime: runtime)
      return try await TeraRuntimeBackendStart(backend: backend, snapshot: backend.snapshot())
    }, deadlines: TeraRuntimeDeadlinePolicy(operationNanoseconds: 3_000_000_000))
  }

  private func checkReconstruction(
    _ client: TeraRuntimeClient, signer: LateNativeSigner, configuration: TeraRuntimeLaunchConfiguration,
    request: TeraSubmissionRequest, retained: TeraSubmissionStatus
  ) async throws {
    _ = try await client.stop()
    await signer.disable()
    _ = try await client.start(configuration: configuration)
    let recovered = try await client.recoverSubmission(request: request)
    XCTAssertEqual(recovered, retained)
    try await checkLocalReconciliation(client, request: request, retained: retained)
    let count = await signer.count()
    XCTAssertEqual(count, 1)
    _ = try await client.stop()
  }

  private func checkLocalReconciliation(_ client: TeraRuntimeClient, request: TeraSubmissionRequest, retained: TeraSubmissionStatus) async throws {
    let context = TeraLocalNetwork(schemaVersion: 1, id: scope.localNetworkID, label: "Nearby",
                                   relayURLs: ["ws://127.0.0.1:19999"], locality: nil, followedAuthors: [], generation: 1)
    let local = try await client.reconcileSubmissionLocal(request: request, context: context)
    XCTAssertEqual(local.captured, retained.captured)
    XCTAssertEqual(local.operationID, retained.operationID)
    XCTAssertEqual(local.settlement.signed, 1)
    XCTAssertEqual(local.settlement.admitted, 1)
    XCTAssertEqual(local.settlement.deliverySatisfied, 0)
    let repeated = try await client.reconcileSubmissionLocal(request: request, context: context)
    XCTAssertEqual(repeated, local)
    let page = try await client.todayPage(request: .first(context: context, limit: 20,
                                                          asOfUnixSeconds: TeraClock.system.unixMilliseconds(requirePositive: true) / 1000))
    XCTAssertEqual(page.items.count, 1)
    XCTAssertEqual(page.items.first?.localOperationID, local.operationID)
    XCTAssertNotEqual(page.items.first?.localOperationState, "complete")
  }

  private func signedStatus(_ client: TeraRuntimeClient, request: TeraSubmissionRequest) async throws -> TeraSubmissionStatus {
    for _ in 0 ..< 100 {
      let status = try await client.submissionStatus(request: request)
      if status.settlement.signed == 1 {
        return status
      }
      try await Task.sleep(nanoseconds: 20_000_000)
    }
    XCTFail("The generated bridge did not retain the already-created signature")
    return try await client.submissionStatus(request: request)
  }

  private var scope: TeraComposerScope {
    TeraComposerScope(authorPublicKey: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798", localNetworkID: "nearby")
  }

  private func configuration(_ fixture: MediaOwnershipFixture, signer: LateNativeSigner) -> TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(applicationSupportDirectory: fixture.root.path, publicKeyHex: scope.authorPublicKey,
                                   sourceGenerationHex: String(repeating: "04", count: 32), sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
                                   protectedData: .available, networkProfile: .publicNetwork, writableRelays: [], blossom: nil,
                                   app: TeraRuntimeAppMetadata(bundleIdentifier: "test.late-signing", version: "1", buildNumber: "1", buildSHA: nil),
                                   signerGeneration: "late-signing-test", signer: signer, adoptBootstrapSettings: false)
  }
}

private actor LateNativeSigner: TeraRuntimeSigner {
  let pause = ResourceTestPause()
  private let signer: any TeraRuntimeSigner
  private var calls = 0
  private var lastDeadline: UInt64?
  private var available = true

  init(signer: any TeraRuntimeSigner) {
    self.signer = signer
  }

  static func make() async throws -> LateNativeSigner {
    let secureStore = InMemorySecureStore()
    let custody = try RadrootsIdentityCustody(configuration: RadrootsIdentityCustodyConfiguration(
      namespace: "radroots_identity_v1", secretPolicy: .secureLocalSecret
    ), secureStore: secureStore, metadataStore: InMemoryIdentityMetadataStore(), userPresence: AllowingUserPresence())
    let store = TeraIdentityStore(custody: custody, secureStore: secureStore,
                                  servicePrefix: "org.tera.tests.late_signing.\(UUID().uuidString)")
    let identity = try await store.importIdentity(RadrootsIdentitySecretMaterial(importText: String(repeating: "0", count: 63) + "1"))
    return try await LateNativeSigner(signer: store.signer(for: identity))
  }

  func availability() async -> TeraRuntimeSignerAvailability {
    available ? .ready : .unavailable
  }

  func count() -> Int {
    calls
  }

  func deadline() -> UInt64? {
    lastDeadline
  }

  func disable() {
    available = false
  }

  func sign(_ request: TeraRuntimeSigningRequest) async -> TeraRuntimeSigningOutcome {
    calls += 1
    guard available else { return .unavailable }
    lastDeadline = request.deadlineUnixMilliseconds
    let result = await signer.sign(request)
    await pause.wait()
    return result
  }
}

private final class LateSigningClock: @unchecked Sendable {
  private let lock = NSLock()
  private var failed = false
  func fail() {
    lock.withLock { failed = true }
  }

  func now() -> Date {
    lock.withLock { failed ? Date(timeIntervalSince1970: .nan) : Date() }
  }
}
