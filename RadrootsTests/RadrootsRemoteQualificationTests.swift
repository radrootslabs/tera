import CryptoKit
@testable import RadrootsApp
import RadrootsKit
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
    XCTAssertEqual(value.runtimeMode, "production")
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
    let presence = RadrootsRemoteQualificationUserPresence()
    let request = try RadrootsUserPresenceRequest(reason: "Automated remote qualification")
    let result = try await presence.verify(request)
    let status = try await presence.currentStatus()
    XCTAssertTrue(result.verified)
    XCTAssertEqual(result.policy, .deviceOwnerAuthentication)
    XCTAssertFalse(status.canEvaluateBiometrics)
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
}
