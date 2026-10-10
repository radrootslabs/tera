import XCTest

final class TeraAccessibilityAuditPolicyUITests: XCTestCase {
  private var capturingAssertion = false
  private var capturedAssertions: [XCTIssue] = []

  override func record(_ issue: XCTIssue) {
    if capturingAssertion, issue.type == .assertionFailure {
      capturedAssertions.append(issue)
    } else {
      super.record(issue)
    }
  }

  @MainActor
  func testEmptyAuditDoesNotAssert() {
    XCTAssertTrue(captureAssertion { TeraAccessibilityAudit.assertNoFindings([]) }.isEmpty)
  }

  @MainActor
  func testUnidentifiedAndClippingFindingsRemainFatal() {
    for label in [nil, "Submit"] as [String?] {
      var findings: [String] = []
      XCTAssertTrue(TeraAccessibilityAudit.retainFinding("Text clipped", elementLabel: label, in: &findings))
      XCTAssertEqual(findings.count, 1)
      let failures = captureAssertion { TeraAccessibilityAudit.assertNoFindings(findings) }
      XCTAssertEqual(failures.count, 1)
      XCTAssertTrue(failures.first?.compactDescription.contains(label ?? "unidentified") == true)
    }
  }

  @MainActor
  func testActiveContrastFindingsRemainFatal() {
    XCTAssertEqual(disposition(label: "Submit", enabled: true), .retain)
    var findings: [String] = []
    XCTAssertTrue(TeraAccessibilityAudit.retainFinding("Contrast", elementLabel: "Submit", in: &findings))
    XCTAssertEqual(captureAssertion { TeraAccessibilityAudit.assertNoFindings(findings) }.count, 1)
  }

  @MainActor
  func testDisabledCameraExceptionCannotMatchEnabledOrOtherControls() {
    XCTAssertEqual(disposition(label: "Camera", enabled: false), .disabledCamera)
    XCTAssertEqual(disposition(label: "Camera", enabled: true), .retain)
    XCTAssertEqual(disposition(label: "Camera", enabled: false, cameraEnabled: true), .retain)
    XCTAssertEqual(disposition(label: "Camera", enabled: false, cameraExists: false), .retain)
    XCTAssertEqual(disposition(label: "Submit", enabled: false), .retain)
    XCTAssertEqual(disposition(label: "Submit", enabled: true), .retain)
    XCTAssertEqual(disposition(label: nil, enabled: false), .retain)
  }

  @MainActor
  func testPartlyOccludedContrastRequiresNewUnobscuredAudit() {
    XCTAssertEqual(disposition(label: "Section", enabled: true, occluded: true), .recheck)
    // The requested target's new audit must retain a continuing finding.
    XCTAssertEqual(disposition(label: "Section", enabled: true, requested: true, occluded: true), .retain)
    XCTAssertEqual(disposition(label: "Section", enabled: true, requested: true), .retain)
    XCTAssertEqual(disposition(label: nil, enabled: false, occluded: true), .retain)
  }

  @MainActor
  func testRecheckExhaustionIsFatal() {
    let failures = captureAssertion { TeraAccessibilityAudit.failContrastRechecks() }
    XCTAssertEqual(failures.count, 1)
    XCTAssertTrue(failures.first?.compactDescription.contains("unobscured recheck") == true)
  }

  @MainActor
  private func captureAssertion(_ action: () -> Void) -> [XCTIssue] {
    // Only this separate synthetic control class captures its injected,
    // synchronous assertion. Native product audits never use this override.
    XCTAssertFalse(capturingAssertion)
    capturedAssertions.removeAll()
    capturingAssertion = true
    defer { capturingAssertion = false }
    action()
    return capturedAssertions
  }

  @MainActor
  private func disposition(label: String?, enabled: Bool, cameraExists: Bool = true,
                           cameraEnabled: Bool = false, requested: Bool = false,
                           occluded: Bool = false) -> TeraAccessibilityContrastDisposition
  {
    TeraAccessibilityAudit.contrastDisposition(label: label, isEnabled: enabled,
                                               cameraIsDisabled: cameraExists && !cameraEnabled,
                                               isRequestedTarget: requested, isPartlyOccluded: occluded)
  }
}
