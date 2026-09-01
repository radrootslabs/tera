import CryptoKit
@testable import RadrootsApp
import RadrootsKit
import RadrootsKitBindings
import XCTest

final class RadrootsRemoteQualificationTests: XCTestCase {
  func testQualificationIsOptInAndCarriesOnlyPublicInputs() throws {
    XCTAssertNil(try RadrootsRemoteQualificationEnvironment.current(environment: [:]))

    let value = try XCTUnwrap(
      RadrootsRemoteQualificationEnvironment.current(
        environment: [
          RadrootsRemoteQualificationEnvironment.enabledKey: "1",
          RadrootsRemoteQualificationEnvironment.runIDKey: "gbrqv1-12345678",
          RadrootsRemoteQualificationEnvironment.relayURLsKey: "wss://write.example",
          RadrootsRemoteQualificationEnvironment.blossomOriginsKey: "https://media.example",
          RadrootsRemoteQualificationEnvironment.mediaRelativePathKey: "qualification/input.png",
          RadrootsRemoteQualificationEnvironment.networkProfileKey: "public",
        ]
      )
    )
    XCTAssertEqual(value.runID, "gbrqv1-12345678")
    XCTAssertEqual(value.relayURLs, ["wss://write.example"])
    XCTAssertEqual(value.blossomOrigins, ["https://media.example"])
    XCTAssertEqual(value.networkMode, .publicEndpoint)
    XCTAssertEqual(value.runtimeMode, "production")
    XCTAssertFalse(value.networkMode.permitsAutomatedUserPresence)
    XCTAssertFalse(value.networkMode.permitsTestSecretPolicy)
    XCTAssertFalse(value.automatesIdentity)
    XCTAssertEqual(
      value.mediaFile,
      RadrootsFileReference(scope: .data, relativePath: "qualification/input.png")
    )
    XCTAssertEqual(
      value.keychainServicePrefix,
      "org.radroots.ios.remote-qualification.gbrqv1-12345678"
    )
    XCTAssertEqual(
      value.identityMetadataKeyPrefix,
      "org.radroots.ios.remote-qualification.gbrqv1-12345678.identity"
    )
  }

  func testQualificationRejectsAmbiguousOrUnsafeHarnessValues() {
    let base = [
      RadrootsRemoteQualificationEnvironment.enabledKey: "1",
      RadrootsRemoteQualificationEnvironment.runIDKey: "gbrqv1-12345678",
      RadrootsRemoteQualificationEnvironment.blossomOriginsKey: "https://media.example",
      RadrootsRemoteQualificationEnvironment.networkProfileKey: "public",
    ]
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationEnvironment.current(
        environment: base.merging([
          RadrootsRemoteQualificationEnvironment.runIDKey: "../../bad",
        ]) { _, new in new }
      )
    )
    for endpoint in [
      "ws://localhost:21000",
      "ws://[::1]:21000",
      "wss://127.0.0.1:21000",
      "ws://127.0.0.1",
      "ws://127.0.0.1:0",
      "ws://user@127.0.0.1:21000",
      "ws://127.0.0.1:21000/path",
      "ws://127.0.0.1:21000?query=1",
    ] {
      XCTAssertThrowsError(
        try RadrootsQualificationEndpoint(
          endpoint,
          role: .relay,
          mode: .isolatedLoopback
        )
      )
    }
    XCTAssertThrowsError(
      try RadrootsQualificationEndpoint(
        "https://127.0.0.1:21100",
        role: .blossom,
        mode: .publicEndpoint
      )
    )
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationEnvironment.current(
        environment: base.merging([
          RadrootsRemoteQualificationEnvironment.networkProfileKey: "simulator",
          RadrootsRemoteQualificationEnvironment.relayURLsKey:
            "ws://127.0.0.1:21000,ws://127.0.0.1:21000",
          RadrootsRemoteQualificationEnvironment.blossomOriginsKey:
            "http://127.0.0.1:21100",
        ]) { _, new in new }
      )
    )
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationEnvironment.current(
        environment: base.merging([
          RadrootsRemoteQualificationEnvironment.networkProfileKey: "automatic",
        ]) { _, new in new }
      )
    )

    let simulator = try? RadrootsRemoteQualificationEnvironment.current(
      environment: base.merging([
        RadrootsRemoteQualificationEnvironment.networkProfileKey: "simulator",
        RadrootsRemoteQualificationEnvironment.relayURLsKey: "ws://127.0.0.1:21000",
        RadrootsRemoteQualificationEnvironment.blossomOriginsKey:
          "http://127.0.0.1:21100",
      ]) { _, new in new }
    )
    XCTAssertEqual(simulator?.runtimeMode, "simulator")
    XCTAssertEqual(simulator?.networkMode, .isolatedLoopback)
    XCTAssertEqual(simulator?.automatesIdentity, true)
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationEnvironment.current(
        environment: base.merging([
          RadrootsRemoteQualificationEnvironment.networkProfileKey: "simulator",
          RadrootsRemoteQualificationEnvironment.relayURLsKey:
            "wss://relay.example",
          RadrootsRemoteQualificationEnvironment.blossomOriginsKey:
            "https://media.example",
        ]) { _, new in new }
      )
    )
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationEnvironment.current(
        environment: base.merging([
          RadrootsRemoteQualificationEnvironment.blossomOriginsKey:
            "https://one.example,https://two.example",
        ]) { _, new in new }
      )
    )
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationEnvironment.current(
        environment: base.merging([
          RadrootsRemoteQualificationEnvironment.mediaRelativePathKey:
            "../../private-key",
        ]) { _, new in new }
      )
    )
  }

  func testQualificationUserPresenceIsNoninteractiveAndDebugOnly() async throws {
    let presence = try RadrootsRemoteQualificationUserPresence(mode: .isolatedLoopback)
    let request = try RadrootsUserPresenceRequest(reason: "Automated remote qualification")
    let result = try await presence.verify(request)
    let status = try await presence.currentStatus()
    XCTAssertTrue(result.verified)
    XCTAssertEqual(result.policy, .deviceOwnerAuthentication)
    XCTAssertFalse(status.canEvaluateBiometrics)
  }

  func testPublicQualificationCannotSelectAutomatedPresence() throws {
    let publicMode = RadrootsQualificationNetworkMode.publicEndpoint
    XCTAssertFalse(publicMode.permitsAutomatedUserPresence)
    XCTAssertFalse(publicMode.permitsTestSecretPolicy)
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationUserPresence(mode: publicMode)
    )
  }

  func testQualificationModeRejectsMutationsWithoutRenderingInput() {
    let canary = "private-key-canary"
    for profile in [nil, "", "SIMULATOR", "public ", "automatic", canary] {
      XCTAssertThrowsError(try RadrootsQualificationNetworkMode(profile: profile)) { error in
        XCTAssertFalse(String(describing: error).contains(canary))
        XCTAssertFalse(error.localizedDescription.contains(canary))
      }
    }
  }

  func testQualificationUsesRunScopedRootsAndBackgroundSession() throws {
    let base = try RadrootsAppleFileRoots(
      appIdentifier: "org.radroots.field-ios",
      dataRoot: URL(fileURLWithPath: "/tmp/data", isDirectory: true),
      cacheRoot: URL(fileURLWithPath: "/tmp/cache", isDirectory: true),
      temporaryRoot: URL(fileURLWithPath: "/tmp/temporary", isDirectory: true)
    )
    let qualification = try XCTUnwrap(
      RadrootsRemoteQualificationEnvironment.current(
        environment: [
          RadrootsRemoteQualificationEnvironment.enabledKey: "1",
          RadrootsRemoteQualificationEnvironment.runIDKey: "persona-p01-12345678",
          RadrootsRemoteQualificationEnvironment.relayURLsKey:
            "ws://127.0.0.1:21000",
          RadrootsRemoteQualificationEnvironment.blossomOriginsKey:
            "http://127.0.0.1:21100",
          RadrootsRemoteQualificationEnvironment.networkProfileKey: "simulator",
        ]
      )
    )
    let isolated = try qualification.isolatedFileRoots(from: base)
    XCTAssertEqual(isolated.dataRoot.path, "/tmp/data/persona-p01-12345678")
    XCTAssertEqual(isolated.cacheRoot.path, "/tmp/cache/persona-p01-12345678")
    XCTAssertEqual(isolated.temporaryRoot.path, "/tmp/temporary/persona-p01-12345678")
    XCTAssertEqual(
      qualification.backgroundTransferIdentifierSuffix,
      "remote-qualification.persona-p01-12345678"
    )
  }

  func testQualificationMediaFixtureHasPinnedDigest() throws {
    let digest = try SHA256.hash(
      data: RadrootsRemoteQualificationEnvironment.mediaFixtureData()
    )
    XCTAssertEqual(
      digest.map { String(format: "%02x", $0) }.joined(),
      "431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460"
    )
  }

  func testAuthorizationEvidenceIsRedactedEphemeralAndRelaunchCleaned() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(
      UUID().uuidString,
      isDirectory: true
    )
    defer { try? FileManager.default.removeItem(at: root) }
    let temporary = root.appendingPathComponent("temporary", isDirectory: true)
    let documents = root.appendingPathComponent("documents", isDirectory: true)
    try FileManager.default.createDirectory(
      at: temporary,
      withIntermediateDirectories: true
    )
    try FileManager.default.createDirectory(
      at: documents,
      withIntermediateDirectories: true
    )
    let store = RadrootsRemoteQualificationEvidenceStore(
      runID: "persona-p01-12345678",
      temporaryRoot: temporary,
      documentsRoot: documents
    )
    try Data("signed-authorization-canary".utf8).write(
      to: store.legacyAuthorizationFile
    )

    try store.prepare()
    XCTAssertFalse(FileManager.default.fileExists(atPath: store.legacyAuthorizationFile.path))
    try store.record(request: qualificationSigningRequest(), signatureHex: String(repeating: "b", count: 128))

    let data = try Data(contentsOf: store.evidenceFile)
    XCTAssertLessThanOrEqual(
      data.count,
      RadrootsRemoteQualificationAuthorizationEvidence.maximumEncodedBytes
    )
    let object = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
    XCTAssertEqual(
      Set(object.keys),
      Set(["schema", "schema_version", "authorization_digest_sha256"])
    )
    XCTAssertEqual(
      object["authorization_digest_sha256"] as? String,
      "fc3a74758a976c6f8dc37a4168520627de9ae8bc6a8433af3064fda94ea3e21b"
    )
    let rendered = try XCTUnwrap(String(bytes: data, encoding: .utf8))
    for forbidden in [
      "signed-authorization-canary", "operation-canary", "artifact-canary",
      "request-canary", "content-canary", String(repeating: "a", count: 64),
      String(repeating: "b", count: 128),
    ] {
      XCTAssertFalse(rendered.contains(forbidden))
    }
    let attributes = try FileManager.default.attributesOfItem(atPath: store.evidenceFile.path)
    XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o600)

    try store.prepare()
    XCTAssertFalse(FileManager.default.fileExists(atPath: store.evidenceFile.path))
    XCTAssertFalse(FileManager.default.fileExists(atPath: store.directory.path))
  }

  func testAuthorizationEvidenceDigestIsDomainSeparatedAndFieldSensitive() {
    let request = qualificationSigningRequest()
    let baseline = RadrootsRemoteQualificationAuthorizationEvidence(
      runID: "persona-p01-12345678",
      request: request,
      signatureHex: String(repeating: "b", count: 128)
    )
    let changedRun = RadrootsRemoteQualificationAuthorizationEvidence(
      runID: "persona-p02-12345678",
      request: request,
      signatureHex: String(repeating: "b", count: 128)
    )
    let changedSignature = RadrootsRemoteQualificationAuthorizationEvidence(
      runID: "persona-p01-12345678",
      request: request,
      signatureHex: String(repeating: "c", count: 128)
    )
    XCTAssertNotEqual(baseline.authorizationDigestSHA256, changedRun.authorizationDigestSHA256)
    XCTAssertNotEqual(
      baseline.authorizationDigestSHA256,
      changedSignature.authorizationDigestSHA256
    )
    XCTAssertTrue(
      RadrootsRemoteQualificationAuthorizationEvidence.digestDomain.hasSuffix("\0")
    )
  }

  func testAuthorizationEvidenceWriteFailureFailsSigningResult() async {
    let signer = RadrootsGeneratedHostSigner(
      signer: QualificationSignedSigner(),
      clock: .fixed(unixSeconds: 1_800_000_000),
      qualificationEvidenceRecorder: { _, _ in
        throw CocoaError(.fileWriteNoPermission)
      }
    )
    let result = await signer.sign(request: qualificationSigningRequest())
    XCTAssertEqual(result.outcome, .failed)
    XCTAssertNil(result.signatureHex)
    XCTAssertEqual(result.completedAtUnixMs, 0)
  }

  private func qualificationSigningRequest() -> HostSigningRequest {
    HostSigningRequest(
      schemaVersion: 1,
      operationKind: "blossom-upload",
      operationId: "operation-canary",
      artifactId: "artifact-canary",
      signerRequestId: "request-canary",
      publicKey: String(repeating: "a", count: 64),
      purpose: .blossomUpload,
      deadlineUnixMs: 1_800_000_300_000,
      eventIdDigest: Data(repeating: 0xAA, count: 32),
      expectedEventId: String(repeating: "a", count: 64),
      createdAtUnixS: 1_800_000_000,
      kind: 24242,
      tags: [["t", "content-canary"]],
      content: "content-canary"
    )
  }
}

private struct QualificationSignedSigner: RadrootsRuntimeSigner {
  func availability() async -> RadrootsRuntimeSignerAvailability {
    .ready
  }

  func sign(_: RadrootsRuntimeSigningRequest) async -> RadrootsRuntimeSigningOutcome {
    .signed(signatureHex: String(repeating: "b", count: 128))
  }
}
