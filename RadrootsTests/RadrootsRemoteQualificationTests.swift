import RadrootsKit
import XCTest

@testable import RadrootsApp

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
          RadrootsRemoteQualificationEnvironment.runIDKey: "../../bad"
        ]) { _, new in new }
      )
    )
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationEnvironment.current(
        environment: base.merging([
          RadrootsRemoteQualificationEnvironment.networkProfileKey: "automatic"
        ]) { _, new in new }
      )
    )

    let simulator = try? RadrootsRemoteQualificationEnvironment.current(
      environment: base.merging([
        RadrootsRemoteQualificationEnvironment.networkProfileKey: "simulator"
      ]) { _, new in new }
    )
    XCTAssertEqual(simulator?.runtimeMode, "simulator")
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationEnvironment.current(
        environment: base.merging([
          RadrootsRemoteQualificationEnvironment.blossomOriginsKey:
            "https://one.example,https://two.example"
        ]) { _, new in new }
      )
    )
    XCTAssertThrowsError(
      try RadrootsRemoteQualificationEnvironment.current(
        environment: base.merging([
          RadrootsRemoteQualificationEnvironment.mediaRelativePathKey:
            "../../private-key"
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
}
