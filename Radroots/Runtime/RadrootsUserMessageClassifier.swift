import Foundation
import RadrootsKit

enum RadrootsUserMessageClassifier {
  static func key(
    for error: Error,
    fallback: RadrootsUserMessageKey
  ) -> RadrootsUserMessageKey {
    runtimeKey(for: error, fallback: fallback)
      ?? platformKey(for: error)
      ?? identityCustodyKey(for: error)
      ?? fallback
  }

  private static func runtimeKey(
    for error: Error,
    fallback: RadrootsUserMessageKey
  ) -> RadrootsUserMessageKey? {
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
    return nil
  }

  private static func identityCustodyKey(for error: Error) -> RadrootsUserMessageKey? {
    error is RadrootsIdentityCustodyError ? .secureStateUnavailable : nil
  }

  private static func platformKey(for error: Error) -> RadrootsUserMessageKey? {
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
    return secondaryPlatformKey(for: error)
  }

  private static func secondaryPlatformKey(
    for error: Error
  ) -> RadrootsUserMessageKey? {
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
    return tertiaryPlatformKey(for: error)
  }

  private static func tertiaryPlatformKey(
    for error: Error
  ) -> RadrootsUserMessageKey? {
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
    return nil
  }

  private static func runtimeClientKey(
    _ error: RadrootsRuntimeClientError,
    fallback: RadrootsUserMessageKey
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidBufferCapacity: .invalidRequest
    case .notRunning: .runtimeUnavailable
    case .superseded: .operationCancelled
    case .startup: .startupFailed
    case .subscription: .runtimeObservationUnavailable
    case .shutdown: .shutdownFailed
    case .status, .today, .add, .support: fallback
    }
  }

  private static func captureKey(
    _ error: RadrootsCaptureIntakeError
  ) -> RadrootsUserMessageKey {
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
    case .unavailable, .preparationFailure: .addMediaUnavailable
    }
  }

  private static func telemetryKey(
    _ error: RadrootsTelemetryError
  ) -> RadrootsUserMessageKey {
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

  private static func fileKey(
    _ error: RadrootsAppleFileError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .notFound, .permanentFailure: .fileOperationFailed
    case .permissionDenied: .permissionDenied
    case .transientFailure: .tryAgain
    }
  }

  private static func locationKey(
    _ error: RadrootsLocationServicesError
  ) -> RadrootsUserMessageKey {
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

  private static func securityKey(
    _ error: RadrootsAppleSecurityError
  ) -> RadrootsUserMessageKey {
    switch error {
    case .invalidRequest: .invalidRequest
    case .permissionDenied: .permissionDenied
    case .userCancelled: .operationCancelled
    case .notFound, .transientFailure, .unavailable, .permanentFailure,
         .keychainFailure:
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
    case .artifactUnavailable, .artifactCorrupt, .fileSystemFailure:
      .fileOperationFailed
    }
  }
}
