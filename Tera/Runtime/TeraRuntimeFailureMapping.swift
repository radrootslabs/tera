import Foundation

extension TeraRuntimeClient {
  static func deadlineFailure(
    identity: TeraRuntimeOperationIdentity
  ) -> TeraRuntimeFailure {
    .local(
      operation: identity.rawValue,
      code: "ios.runtime.deadline_exceeded",
      safeMessage: "The Tera runtime operation did not finish in time."
    )
  }

  static func cancellationFailure(
    identity: TeraRuntimeOperationIdentity
  ) -> TeraRuntimeFailure {
    .local(
      operation: identity.rawValue,
      code: "ios.runtime.cancelled",
      safeMessage: "The Tera runtime operation was cancelled."
    )
  }

  static func failure(from error: Error, operation: String) -> TeraRuntimeFailure {
    if let failure = error as? TeraRuntimeFailure {
      return failure
    }
    if error is CancellationError {
      return .local(
        operation: operation,
        code: "ios.runtime.cancelled",
        safeMessage: "The Tera runtime operation was cancelled."
      )
    }
    if case let TeraRuntimeClientError.startup(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.subscription(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.status(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.today(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.add(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.support(failure) = error {
      return failure
    }
    if case let TeraRuntimeClientError.shutdown(failure) = error {
      return failure
    }
    return .local(
      operation: operation,
      code: "ios.runtime.unexpected",
      safeMessage: "The Tera runtime could not complete the operation."
    )
  }
}
