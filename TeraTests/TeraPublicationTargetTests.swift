@testable import TeraApp
import TeraKitBindings
import XCTest

extension TeraPublicationTargets {
  static func fixture(accepted: Bool = false) -> Self {
    Self(requiresDelivery: false, policy: .any, targets: [
      TeraPublicationTarget(id: String(repeating: "a", count: 64), endpoint: "wss://relay.example",
                            attempted: accepted, accepted: accepted, delivered: false, rejected: false,
                            uncertain: false, readBackObservedAtUnixMilliseconds: nil),
    ], readBackAvailable: true, readBackComplete: true)
  }
}

final class TeraPublicationTargetTests: XCTestCase {
  private func wire() -> FfiPublicationTargetDetails {
    FfiPublicationTargetDetails(requiresDelivery: false, policy: .any, targets: [
      FfiPublicationTargetEvidence(fingerprint: String(repeating: "a", count: 64), endpoint: "wss://a.example",
                                   attempted: true, accepted: true, delivered: false, rejected: false, uncertain: false,
                                   readBackObservedAtUnixMs: nil),
      FfiPublicationTargetEvidence(fingerprint: String(repeating: "b", count: 64), endpoint: "wss://b.example",
                                   attempted: true, accepted: false, delivered: false, rejected: true, uncertain: false,
                                   readBackObservedAtUnixMs: nil),
      FfiPublicationTargetEvidence(fingerprint: String(repeating: "c", count: 64), endpoint: "wss://c.example",
                                   attempted: true, accepted: false, delivered: false, rejected: false, uncertain: true,
                                   readBackObservedAtUnixMs: 100),
    ], readBackAvailable: true, readBackComplete: true)
  }

  func testMixedReceiptsAndReadBackStaySeparate() throws {
    let value = try TeraPublicationTargets.decode(wire())
    XCTAssertEqual(value.policy, .any)
    XCTAssertTrue(value.policySummary.contains("any one"))
    XCTAssertEqual(value.targets.filter(\.accepted).count, 1)
    XCTAssertTrue(value.targets[1].summary.contains("refusal"))
    XCTAssertTrue(value.targets[2].summary.contains("uncertain"))
    XCTAssertEqual(value.targets[2].readBackObservedAtUnixMilliseconds, 100)
    XCTAssertFalse(value.targets[2].accepted)
    XCTAssertTrue(value.follows(value))
  }

  func testMalformedTargetsAndPolicyFailClosed() {
    var value = wire()
    value.targets.append(value.targets[0])
    XCTAssertThrowsError(try TeraPublicationTargets.decode(value))
    value = wire(); value.targets[0].attempted = false
    XCTAssertThrowsError(try TeraPublicationTargets.decode(value))
    value = wire(); value.policy = .quorum(threshold: 4)
    XCTAssertThrowsError(try TeraPublicationTargets.decode(value))
    value = wire(); value.policy = .required(fingerprints: [String(repeating: "d", count: 64)])
    XCTAssertThrowsError(try TeraPublicationTargets.decode(value))
    value = wire(); value.readBackAvailable = false
    XCTAssertThrowsError(try TeraPublicationTargets.decode(value))
    value = wire(); value.targets[0].fingerprint = "bad"
    XCTAssertThrowsError(try TeraPublicationTargets.decode(value))
  }

  func testLaterStatusCannotEraseAcceptanceOrChangeSavedDestinations() throws {
    let original = try TeraPublicationTargets.decode(wire())
    var value = wire(); value.targets[0].accepted = false
    XCTAssertFalse(try TeraPublicationTargets.decode(value).follows(original))
    value = wire(); value.targets.removeLast()
    XCTAssertFalse(try TeraPublicationTargets.decode(value).follows(original))
    value = wire(); value.policy = .all
    XCTAssertFalse(try TeraPublicationTargets.decode(value).follows(original))
    value = wire(); value.readBackComplete = false
    XCTAssertTrue(try TeraPublicationTargets.decode(value).follows(original))
  }
}
