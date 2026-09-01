import Foundation
@testable import RadrootsApp
import RadrootsKit
import XCTest

final class RadrootsUserMessagesTests: XCTestCase {
  func testMessageKeyInventoryIsClosedAndLocalized() {
    XCTAssertEqual(
      Set(RadrootsUserMessageKey.allCases.map(\.rawValue)),
      [
        "error.add.media_unavailable",
        "error.add.operation_failed",
        "error.background_transfer.failed",
        "error.background_transfer.unavailable",
        "error.configuration.invalid",
        "error.diagnostics.export_failed",
        "error.diagnostics.prepare_failed",
        "error.file.operation_failed",
        "error.identity.operation_failed",
        "error.location.unavailable",
        "error.operation.cancelled",
        "error.operation.try_again",
        "error.permission.denied",
        "error.profile.unavailable",
        "error.request.invalid",
        "error.runtime.observation_unavailable",
        "error.runtime.operation_failed",
        "error.runtime.unavailable",
        "error.search.unavailable",
        "error.secure_state.unavailable",
        "error.settings.operation_failed",
        "error.shutdown.failed",
        "error.startup.failed",
        "error.today.changed",
        "error.today.select_network",
        "error.today.unavailable",
        "error.user_presence.unavailable",
        "status.diagnostics.export_succeeded",
      ]
    )
    for key in RadrootsUserMessageKey.allCases {
      XCTAssertFalse(RadrootsUserMessages.text(key).isEmpty, key.rawValue)
    }
  }

  func testAppleErrorsMapToClosedUserMessageKeys() {
    let vectors: [(Error, RadrootsUserMessageKey)] = [
      (RadrootsCaptureIntakeError.invalidRequest, .invalidRequest),
      (RadrootsCaptureIntakeError.unavailable, .addMediaUnavailable),
      (RadrootsCaptureIntakeError.permissionDenied, .permissionDenied),
      (RadrootsCaptureIntakeError.userCancelled, .operationCancelled),
      (RadrootsCaptureIntakeError.transientFailure, .tryAgain),
      (RadrootsCaptureIntakeError.permanentFailure, .addMediaUnavailable),
      (RadrootsBackgroundTransferError.invalidRequest, .invalidRequest),
      (RadrootsBackgroundTransferError.unavailable, .backgroundTransferUnavailable),
      (RadrootsBackgroundTransferError.transferFailure, .backgroundTransferFailed),
      (RadrootsBackgroundTransferError.persistenceFailure, .secureStateUnavailable),
      (RadrootsDocumentInterchangeError.notFound, .fileOperationFailed),
      (RadrootsDocumentInterchangeError.userCancelled, .operationCancelled),
      (RadrootsDocumentInterchangeError.permissionDenied, .permissionDenied),
      (RadrootsAppLocalStateResetError.keychainFailure, .secureStateUnavailable),
      (RadrootsAppleMediaPreparationError.preparationFailure, .addMediaUnavailable),
      (RadrootsTelemetryError.invalidRequest, .invalidRequest),
      (RadrootsExternalActionError.blockedByPolicy, .permissionDenied),
      (RadrootsExternalActionError.transientFailure, .tryAgain),
      (RadrootsAppleFileError.notFound, .fileOperationFailed),
      (RadrootsAppleFileError.permissionDenied, .permissionDenied),
      (RadrootsLocationServicesError.permissionDenied, .permissionDenied),
      (RadrootsLocationServicesError.timeout, .locationUnavailable),
      (RadrootsUserPresenceError.userCancelled, .operationCancelled),
      (RadrootsUserPresenceError.unavailable, .userPresenceUnavailable),
      (RadrootsBackgroundTaskError.schedulerFailure, .runtimeUnavailable),
      (RadrootsAppleSecurityError.userCancelled, .operationCancelled),
      (RadrootsAppleSecurityError.keychainFailure, .secureStateUnavailable),
      (RadrootsAppleMobileStoreError.invalidPublicKey, .invalidRequest),
      (RadrootsAppleMobileStoreError.fileSystemFailure, .secureStateUnavailable),
      (RadrootsVerifiedArtifactAccessError.invalidDescriptor, .invalidRequest),
      (RadrootsVerifiedArtifactAccessError.artifactCorrupt, .fileOperationFailed),
      (RadrootsIdentityCustodyError.cryptographyFailed, .secureStateUnavailable),
    ]

    for (error, expected) in vectors {
      XCTAssertEqual(
        RadrootsUserMessages.key(for: error, fallback: .runtimeOperationFailed),
        expected
      )
    }
  }

  func testDependencyDiagnosticsAndRuntimeSafeMessagesCannotReachUserText() {
    let canaries = [
      "nsec1step264canary",
      "/Users/private/step264.sqlite",
      "wss://private.invalid/relay",
    ]
    let errors: [Error] = [
      NSError(
        domain: canaries[1],
        code: 1,
        userInfo: [NSLocalizedDescriptionKey: canaries.joined(separator: " ")]
      ),
      RadrootsRuntimeFailure.local(
        operation: canaries[2],
        code: "private.failure",
        safeMessage: canaries[0]
      ),
      RadrootsConfigurationError.missing(canaries[1]),
      RadrootsIdentityStoreError.custody(canaries[0]),
    ]

    for error in errors {
      let rendered = RadrootsUserMessages.text(for: error, fallback: .runtimeOperationFailed)
      XCTAssertFalse(rendered.isEmpty)
      for canary in canaries {
        XCTAssertFalse(rendered.contains(canary))
      }
    }
  }
}
