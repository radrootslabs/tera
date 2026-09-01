import Foundation
import RadrootsKit

enum RadrootsUserMessageKey: String, CaseIterable, Sendable {
  case addMediaUnavailable = "error.add.media_unavailable"
  case addOperationFailed = "error.add.operation_failed"
  case backgroundTransferFailed = "error.background_transfer.failed"
  case backgroundTransferUnavailable = "error.background_transfer.unavailable"
  case configurationInvalid = "error.configuration.invalid"
  case diagnosticsExportFailed = "error.diagnostics.export_failed"
  case diagnosticsExportSucceeded = "status.diagnostics.export_succeeded"
  case diagnosticsPrepareFailed = "error.diagnostics.prepare_failed"
  case fileOperationFailed = "error.file.operation_failed"
  case identityOperationFailed = "error.identity.operation_failed"
  case invalidRequest = "error.request.invalid"
  case locationUnavailable = "error.location.unavailable"
  case operationCancelled = "error.operation.cancelled"
  case permissionDenied = "error.permission.denied"
  case profileUnavailable = "error.profile.unavailable"
  case runtimeObservationUnavailable = "error.runtime.observation_unavailable"
  case runtimeOperationFailed = "error.runtime.operation_failed"
  case runtimeUnavailable = "error.runtime.unavailable"
  case searchUnavailable = "error.search.unavailable"
  case secureStateUnavailable = "error.secure_state.unavailable"
  case settingsOperationFailed = "error.settings.operation_failed"
  case shutdownFailed = "error.shutdown.failed"
  case startupFailed = "error.startup.failed"
  case todayChanged = "error.today.changed"
  case todaySelectNetwork = "error.today.select_network"
  case todayUnavailable = "error.today.unavailable"
  case tryAgain = "error.operation.try_again"
  case userPresenceUnavailable = "error.user_presence.unavailable"

  var defaultValue: String {
    switch self {
    case .addMediaUnavailable:
      "Photo handling is unavailable."
    case .addOperationFailed:
      "The Add operation could not be completed."
    case .backgroundTransferFailed:
      "The background transfer did not complete."
    case .backgroundTransferUnavailable:
      "Background transfer is unavailable."
    case .configurationInvalid:
      "Radroots configuration is invalid."
    case .diagnosticsExportFailed:
      "Diagnostics export was cancelled or failed."
    case .diagnosticsExportSucceeded:
      "Diagnostics exported."
    case .diagnosticsPrepareFailed:
      "Diagnostics export could not be prepared."
    case .fileOperationFailed:
      "The file operation could not be completed."
    case .identityOperationFailed:
      "The local identity operation could not be completed."
    case .invalidRequest:
      "The request is invalid."
    case .locationUnavailable:
      "Location is unavailable."
    case .operationCancelled:
      "The operation was cancelled."
    case .permissionDenied:
      "Permission was denied."
    case .profileUnavailable:
      "Your profile is unavailable."
    case .runtimeObservationUnavailable:
      "Runtime observation is temporarily unavailable."
    case .runtimeOperationFailed:
      "The Radroots runtime could not complete the operation."
    case .runtimeUnavailable:
      "The Radroots runtime is unavailable."
    case .searchUnavailable:
      "Search is unavailable."
    case .secureStateUnavailable:
      "Secure local state is unavailable."
    case .settingsOperationFailed:
      "The requested settings operation could not be completed."
    case .shutdownFailed:
      "Radroots could not finish shutting down."
    case .startupFailed:
      "Radroots could not start."
    case .todayChanged:
      "Today changed while loading. Refresh to continue."
    case .todaySelectNetwork:
      "Choose a local network to load Today."
    case .todayUnavailable:
      "Today could not be loaded."
    case .tryAgain:
      "The operation could not be completed temporarily."
    case .userPresenceUnavailable:
      "User verification is unavailable."
    }
  }
}

enum RadrootsUserMessages {
  static func text(_ key: RadrootsUserMessageKey, bundle: Bundle? = nil) -> String {
    let bundle = bundle ?? defaultBundle
    return bundle.localizedString(forKey: key.rawValue, value: key.defaultValue, table: nil)
  }

  static func text(
    for error: Error,
    fallback: RadrootsUserMessageKey,
    bundle: Bundle? = nil
  ) -> String {
    text(key(for: error, fallback: fallback), bundle: bundle)
  }

  static func key(
    for error: Error,
    fallback: RadrootsUserMessageKey
  ) -> RadrootsUserMessageKey {
    if error is CancellationError {
      return .operationCancelled
    }
    if let error = error as? RadrootsRuntimeClientError {
      return runtimeClientKey(error, fallback: fallback)
    }
    if error is RadrootsRuntimeFailure {
      return fallback
    }
    if error is RadrootsConfigurationError {
      return .configurationInvalid
    }
    if error is RadrootsIdentityStoreError {
      return .identityOperationFailed
    }
    if let error = error as? RadrootsCaptureIntakeError {
      return captureKey(error)
    }
    if let error = error as? RadrootsBackgroundTransferError {
      return backgroundTransferKey(error)
    }
    if let error = error as? RadrootsDocumentInterchangeError {
      return documentKey(error)
    }
    if let error = error as? RadrootsAppLocalStateResetError {
      return localStateResetKey(error)
    }
    if let error = error as? RadrootsAppleMediaPreparationError {
      return mediaPreparationKey(error)
    }
    if let error = error as? RadrootsTelemetryError {
      return telemetryKey(error)
    }
    if let error = error as? RadrootsExternalActionError {
      return externalActionKey(error)
    }
    if let error = error as? RadrootsAppleFileError {
      return fileKey(error)
    }
    if let error = error as? RadrootsLocationServicesError {
      return locationKey(error)
    }
    if let error = error as? RadrootsUserPresenceError {
      return userPresenceKey(error)
    }
    if let error = error as? RadrootsBackgroundTaskError {
      return backgroundTaskKey(error)
    }
    if let error = error as? RadrootsAppleSecurityError {
      return securityKey(error)
    }
    if let error = error as? RadrootsAppleMobileStoreError {
      return mobileStoreKey(error)
    }
    if let error = error as? RadrootsVerifiedArtifactAccessError {
      return verifiedArtifactKey(error)
    }
    if error is RadrootsIdentityCustodyError {
      return .secureStateUnavailable
    }
    return fallback
  }

  private static func runtimeClientKey(
    _ error: RadrootsRuntimeClientError,
    fallback: RadrootsUserMessageKey
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidBufferCapacity:
      .invalidRequest
    case .notRunning:
      .runtimeUnavailable
    case .superseded:
      .operationCancelled
    case .startup:
      .startupFailed
    case .subscription:
      .runtimeObservationUnavailable
    case .shutdown:
      .shutdownFailed
    case .status, .today, .add, .support:
      fallback
    }
  }

  private static func captureKey(_ error: RadrootsCaptureIntakeError) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .unavailable: .addMediaUnavailable
    case .permissionDenied: .permissionDenied
    case .userCancelled: .operationCancelled
    case .transientFailure: .tryAgain
    case .permanentFailure: .addMediaUnavailable
    }
  }

  private static func backgroundTransferKey(
    _ error: RadrootsBackgroundTransferError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .unavailable: .backgroundTransferUnavailable
    case .transferFailure: .backgroundTransferFailed
    case .persistenceFailure: .secureStateUnavailable
    }
  }

  private static func documentKey(
    _ error: RadrootsDocumentInterchangeError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .notFound: .fileOperationFailed
    case .userCancelled: .operationCancelled
    case .permissionDenied: .permissionDenied
    case .transientFailure: .tryAgain
    case .permanentFailure: .fileOperationFailed
    }
  }

  private static func localStateResetKey(
    _ error: RadrootsAppLocalStateResetError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .fileSystemFailure, .keychainFailure: .secureStateUnavailable
    }
  }

  private static func mediaPreparationKey(
    _ error: RadrootsAppleMediaPreparationError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .unavailable: .addMediaUnavailable
    case .preparationFailure: .addMediaUnavailable
    }
  }

  private static func telemetryKey(_ error: RadrootsTelemetryError) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    }
  }

  private static func externalActionKey(
    _ error: RadrootsExternalActionError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .blockedByPolicy, .unavailable: .permissionDenied
    case .transientFailure: .tryAgain
    case .permanentFailure: .runtimeOperationFailed
    }
  }

  private static func fileKey(_ error: RadrootsAppleFileError) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .notFound, .permanentFailure: .fileOperationFailed
    case .permissionDenied: .permissionDenied
    case .transientFailure: .tryAgain
    }
  }

  private static func locationKey(_ error: RadrootsLocationServicesError) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .permissionDenied: .permissionDenied
    case .unavailable, .timeout, .cancelled, .transientFailure, .permanentFailure:
      .locationUnavailable
    }
  }

  private static func userPresenceKey(
    _ error: RadrootsUserPresenceError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .userCancelled: .operationCancelled
    case .permissionDenied: .permissionDenied
    case .unavailable, .timeout, .transientFailure, .permanentFailure:
      .userPresenceUnavailable
    }
  }

  private static func backgroundTaskKey(
    _ error: RadrootsBackgroundTaskError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .unavailable, .schedulerFailure: .runtimeUnavailable
    }
  }

  private static func securityKey(_ error: RadrootsAppleSecurityError) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .permissionDenied: .permissionDenied
    case .userCancelled: .operationCancelled
    case .notFound, .transientFailure, .unavailable, .permanentFailure, .keychainFailure:
      .secureStateUnavailable
    }
  }

  private static func mobileStoreKey(
    _ error: RadrootsAppleMobileStoreError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidPublicKey: .invalidRequest
    case .protectedDataUnavailable, .invalidDirectoryLayout, .fileSystemFailure:
      .secureStateUnavailable
    }
  }

  private static func verifiedArtifactKey(
    _ error: RadrootsVerifiedArtifactAccessError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidDescriptor: .invalidRequest
    case .protectedDataUnavailable: .secureStateUnavailable
    case .artifactUnavailable, .artifactCorrupt, .fileSystemFailure: .fileOperationFailed
    }
  }

  private static var defaultBundle: Bundle {
    #if SWIFT_PACKAGE
      Bundle.module
    #else
      Bundle.main
    #endif
  }
}
