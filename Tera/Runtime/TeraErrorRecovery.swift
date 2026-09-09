import Foundation
import TeraKitBindings

extension TeraRuntimeFailure {
  var recovery: FfiRecoveryDecision {
    classifyErrorRecovery(schemaVersion: schemaVersion, code: code)
  }

  static func from(_ error: Error) -> TeraRuntimeFailure? {
    if let failure = error as? TeraRuntimeFailure {
      return failure
    }
    guard let error = error as? TeraRuntimeClientError else { return nil }
    switch error {
    case let .startup(failure), let .subscription(failure), let .status(failure),
         let .today(failure), let .add(failure), let .support(failure), let .shutdown(failure):
      return failure
    case .invalidBufferCapacity, .notRunning, .superseded:
      return nil
    }
  }

  func messageKey(fallback: TeraUserMessageKey) -> TeraUserMessageKey {
    Self.recoveryMessages[recovery.disposition] ?? fallback
  }

  private static let recoveryMessages: [FfiRecoveryDisposition: TeraUserMessageKey] = [
    .invalidInput: .invalidRequest,
    .staleRevision: .stateChanged,
    .idempotencyConflict: .operationConflict,
    .staleCursor: .todayChanged,
    .protectedDataUnavailable: .protectedDataUnavailable,
    .identityUnavailable: .identityOperationFailed,
    .storageFailure: .secureStateUnavailable,
    .quotaExhausted: .storageFull,
    .cancelledBeforeEffect: .operationCancelled,
    .outcomeUnknown: .operationOutcomeUnknown,
    .unsupportedVersion: .versionUnsupported,
    .networkUnavailable: .networkUnavailable,
    .partialResult: .partialResult,
    .networkPolicy: .networkPolicy,
    .mediaCorrupt: .fileOperationFailed,
    .runtimeUnavailable: .runtimeUnavailable,
  ]
}
