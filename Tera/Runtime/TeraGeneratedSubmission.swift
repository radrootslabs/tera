import Foundation
import TeraKitBindings

enum TeraGeneratedSubmission {
  static func reserveID() throws -> String {
    let value = try submissionReserveId()
    try version(value.schemaVersion)
    guard validID(value.id) else { throw mismatch() }
    return value.id
  }

  static func reserve(runtime: TeraRuntime, request: TeraSubmissionRequest) async throws -> TeraSubmissionReservation {
    let value = try await runtime.submissionReserve(request: FfiSubmissionReservationRequest(
      schemaVersion: 1, commandId: request.commandID, scope: request.scope.generatedValue,
      composerId: request.composerID, expectedRevision: request.expectedRevision
    ))
    return try translate(value, request: request)
  }

  static func translate(_ value: FfiSubmissionReservationReceipt, request: TeraSubmissionRequest) throws -> TeraSubmissionReservation {
    try version(value.schemaVersion)
    let captured = try value.captured.composerAppValue
    guard value.commandId == request.commandID, validID(value.commandId), validID(value.reservationId),
          captured.scope == request.scope, captured.id == request.composerID,
          captured.revision == request.expectedRevision,
          value.reservedAtUnixMs > 0, value.reservedAtUnixMs <= UInt64(Int64.max)
    else { throw mismatch() }
    return TeraSubmissionReservation(commandID: value.commandId, reservationID: value.reservationId,
                                     captured: captured, reservedAtUnixMilliseconds: value.reservedAtUnixMs,
                                     replayed: value.replayed)
  }

  private static func validID(_ value: String) -> Bool {
    value.utf8.count == 32 && value != String(repeating: "0", count: 32)
      && value.utf8.allSatisfy { (48 ... 57).contains($0) || (97 ... 102).contains($0) }
  }

  private static func version(_ version: UInt16) throws {
    guard version == 1 else {
      throw TeraRuntimeFailure.local(operation: "runtime.submission", code: "submission_schema_unsupported",
                                     safeMessage: "This reservation format requires a compatible app.")
    }
  }

  private static func mismatch() -> TeraRuntimeFailure {
    .local(operation: "runtime.submission", code: "submission_receipt_mismatch",
           safeMessage: "The reservation response could not be reconciled.")
  }
}
