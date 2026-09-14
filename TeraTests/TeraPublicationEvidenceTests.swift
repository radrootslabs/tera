@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraPublicationEvidenceTests: XCTestCase {
  func testLateCallbacksCannotEraseFirstStopAcceptanceOrDurableCounters() {
    let before = evidence(.unknown)
    let stopped = evidence(.unknown, stop: 10, revision: 3)
    let accepted = evidence(.accepted, stop: 10, revision: 3, facts: 1)
    XCTAssertTrue(stopped.follows(before))
    XCTAssertTrue(accepted.follows(stopped))
    XCTAssertFalse(before.follows(stopped))
    XCTAssertFalse(stopped.follows(accepted))
    XCTAssertFalse(evidence(.accepted, stop: 11, revision: 4, facts: 1).follows(accepted))
    XCTAssertFalse(evidence(.accepted, stop: 10, revision: 2, facts: 1).follows(accepted))
    XCTAssertFalse(evidence(.accepted, stop: 10, revision: 4).follows(accepted))
    XCTAssertFalse(evidence(.notIssued).follows(before))
    XCTAssertFalse(before.follows(evidence(.partiallyAccepted)))
    XCTAssertTrue(accepted.follows(evidence(.partiallyAccepted)))
  }

  func testGeneratedEvidenceRejectsInvalidBoundsAndFalseAbsenceClaims() throws {
    let valid = FfiPublicationDeliveryEvidence(state: .notIssued, stopRequestedAtUnixMs: 10,
                                               schedulingRevision: 2, retainedFacts: 0, recordedAttempts: 0, unresolvedClaims: false)
    XCTAssertEqual(try TeraPublicationEvidence.decode(valid).state, .notIssued)
    var invalid = valid
    invalid.unresolvedClaims = true
    XCTAssertThrowsError(try TeraPublicationEvidence.decode(invalid))
    invalid = valid; invalid.retainedFacts = 1
    XCTAssertThrowsError(try TeraPublicationEvidence.decode(invalid))
    invalid = valid; invalid.recordedAttempts = 1
    XCTAssertThrowsError(try TeraPublicationEvidence.decode(invalid))
    invalid = valid; invalid.schedulingRevision = 0
    XCTAssertThrowsError(try TeraPublicationEvidence.decode(invalid))
    invalid = valid; invalid.stopRequestedAtUnixMs = 0
    XCTAssertThrowsError(try TeraPublicationEvidence.decode(invalid))
    invalid = valid; invalid.state = .unknown; invalid.retainedFacts = 1025
    XCTAssertThrowsError(try TeraPublicationEvidence.decode(invalid))
  }

  private func evidence(_ state: TeraPublicationDeliveryState, stop: UInt64? = nil,
                        revision: UInt64 = 2, facts: UInt32 = 0) -> TeraPublicationEvidence
  {
    TeraPublicationEvidence(state: state, stopRequestedAtUnixMilliseconds: stop, schedulingRevision: revision,
                            retainedFacts: facts, recordedAttempts: 0, unresolvedClaims: state == .unknown)
  }
}
