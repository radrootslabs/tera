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
    RadrootsUserMessageClassifier.key(for: error, fallback: fallback)
  }

  private static var defaultBundle: Bundle {
    #if SWIFT_PACKAGE
      Bundle.module
    #else
      Bundle.main
    #endif
  }
}
