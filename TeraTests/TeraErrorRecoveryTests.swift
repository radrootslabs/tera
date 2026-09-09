import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraErrorRecoveryTests: XCTestCase {
  @MainActor
  func testStableCodesDriveDistinctMessagesAndHonestStoreStates() {
    let cases: [(String, TeraUserMessageKey)] = [
      ("database_busy", .secureStateUnavailable),
      ("identity_unavailable", .identityOperationFailed),
      ("protected_data_unavailable", .protectedDataUnavailable),
      ("storage_space_insufficient", .storageFull),
      ("schema_too_new", .versionUnsupported),
      ("cancelled_no_commit", .operationCancelled),
      ("ios.runtime.cancelled", .operationOutcomeUnknown),
      ("ios.runtime.deadline_exceeded", .operationOutcomeUnknown),
      ("idempotency_conflict", .operationConflict),
      ("draft_revision_conflict", .stateChanged),
      ("today_cursor_invalid", .todayChanged),
      ("relay_auth_required", .networkPolicy),
      ("relay_rate_limited", .networkPolicy),
      ("today_relay_partial", .partialResult),
      ("future_corrupt_network", .todayUnavailable),
    ]
    for (code, key) in cases {
      let failure = makeFailure(code: code)
      let wrapped = TeraRuntimeClientError.today(failure)
      XCTAssertEqual(TeraUserMessages.key(for: wrapped, fallback: .todayUnavailable), key, code)
      XCTAssertEqual(
        TeraTodayStore.failureState(wrapped),
        .failed(message: TeraUserMessages.text(key)), code
      )
      XCTAssertEqual(TeraMediaStore.failureState(wrapped), .failed, code)
    }
    let offline = makeFailure(code: "today_relay_offline")
    XCTAssertEqual(
      TeraTodayStore.failureState(offline),
      .offline(message: TeraUserMessages.text(.networkUnavailable))
    )
    XCTAssertEqual(
      TeraMediaStore.failureState(makeFailure(code: "blossom_transport_failed")),
      .networkUnavailable
    )
    XCTAssertEqual(TeraMediaStore.failureState(makeFailure(code: "today_media_corrupt")), .corrupt)
  }

  func testDiagnosticTextAndClaimedRetryActionsCannotChangeRecovery() {
    for code in ["database_busy", "today_relay_offline", "future_corrupt_network"] {
      let first = makeFailure(code: code)
      let second = TeraRuntimeFailure(
        schemaVersion: 1, code: code, category: "storage", retryable: false,
        recoveryActions: [], operationID: "another-diagnostic", capabilityID: nil,
        safeMessage: "Un texte traduit sans indication de réseau."
      )
      XCTAssertEqual(first.recovery, second.recovery)
      XCTAssertEqual(
        TeraUserMessages.key(for: first, fallback: .todayUnavailable),
        TeraUserMessages.key(for: second, fallback: .todayUnavailable)
      )
      XCTAssertFalse(TeraUserMessages.text(for: first, fallback: .todayUnavailable).contains(first.safeMessage))
    }
    let unknown = makeFailure(code: "future_corrupt_network")
    XCTAssertEqual(unknown.recovery.disposition, .unknown)
    XCTAssertEqual(unknown.recovery.retry, .notAllowed)
    let unsupported = makeFailure(code: "today_relay_offline", schemaVersion: 2)
    XCTAssertEqual(unsupported.recovery.disposition, .unsupportedVersion)
    XCTAssertEqual(unsupported.recovery.retry, .notAllowed)
  }

  func testCallerCancellationAndTimeoutRequireReconciliation() {
    for code in ["ios.runtime.cancelled", "ios.runtime.deadline_exceeded", "deadline_exceeded"] {
      XCTAssertEqual(makeFailure(code: code).recovery.retry, .reconcileExistingOperation)
    }
    XCTAssertEqual(
      TeraUserMessages.key(for: CancellationError(), fallback: .addOperationFailed),
      .operationOutcomeUnknown
    )
    XCTAssertEqual(makeFailure(code: "cancelled_no_commit").recovery.disposition, .cancelledBeforeEffect)
  }

  private func makeFailure(code: String, schemaVersion: UInt16 = 1) -> TeraRuntimeFailure {
    TeraRuntimeFailure(
      schemaVersion: schemaVersion, code: code, category: "network relay corrupt",
      retryable: true, recoveryActions: ["publish_again"], operationID: "existing-operation",
      capabilityID: "transport", safeMessage: "Texte localisé: offline retry corruption"
    )
  }
}
