import Foundation
import TeraKitBindings

enum TeraGeneratedRuntimeFailure {
  static func from(_ error: Error) -> TeraRuntimeFailure {
    if case let TeraAppError.Failure(report) = error {
      return TeraRuntimeFailure(
        schemaVersion: report.schemaVersion,
        code: report.code,
        category: report.category,
        retryable: report.retryable,
        recoveryActions: report.recoveryActions,
        operationID: report.operationId,
        capabilityID: report.capabilityId,
        safeMessage: report.safeMessage
      )
    }
    if let failure = error as? TeraRuntimeFailure {
      return failure
    }
    return .local(
      operation: "generated.runtime",
      code: "ios.generated_runtime.unexpected",
      safeMessage: "The Tera runtime could not complete the operation."
    )
  }
}
