import Foundation
@testable import TeraApp

extension AddBackend {
  func saveRevisionIntent(
    requestID: String,
    target: TeraRevisionTarget,
    replacement: TeraAddRuntimeInput
  ) throws -> TeraRevisionStatus {
    if values[requestID] != nil {
      return try revisionStatus(operationID: requestID)
    }
    revisionPlans += 1
    let id = requestID
    let status = makeStatus(
      id: id,
      revision: 1,
      kind: .add,
      commandType: replacement.form.commandType,
      form: replacement.form,
      state: replacement.form.media.isEmpty ? .draft : .mediaPreparing,
      updatedAt: 1_800_000_100_000,
      media: [],
      isRevision: true
    )
    values[id] = status
    if failRevisionReceipt {
      failRevisionReceipt = false
      throw unsupported()
    }
    return TeraRevisionStatus(
      operationID: id,
      replacement: status,
      retraction: nil,
      policy: target.sourceAddress == nil ? .replaceThenRetract : .addressableReplacement,
      phase: .replacementPending
    )
  }

  func revisionSourceForm(card _: TeraTodayCard, sourceDraftID: String) throws -> TeraAddForm {
    guard let form = try draftStatus(id: sourceDraftID).form else { throw unsupported() }
    return form
  }

  func revisionStatus(operationID: String) throws -> TeraRevisionStatus {
    let replacement = try draftStatus(id: operationID)
    return TeraRevisionStatus(
      operationID: operationID,
      replacement: replacement,
      retraction: nil,
      policy: .addressableReplacement,
      phase: replacement.state == .complete ? .complete : .replacementPending
    )
  }

  func advanceRevision(operationID: String) throws -> TeraRevisionStatus {
    let current = try draftStatus(id: operationID)
    let completed = replacing(
      current,
      revision: current.revision + 1,
      state: .complete,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[operationID] = completed
    return try revisionStatus(operationID: operationID)
  }

  func cancelRevision(operationID: String) throws -> TeraRevisionStatus {
    let current = try draftStatus(id: operationID)
    let cancelled = replacing(
      current,
      revision: current.revision + 1,
      state: .cancelled,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[operationID] = cancelled
    return TeraRevisionStatus(
      operationID: operationID,
      replacement: cancelled,
      retraction: nil,
      policy: .addressableReplacement,
      phase: .cancelled
    )
  }
}
