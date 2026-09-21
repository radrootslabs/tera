import TeraKitBindings

extension FfiRevisionStatusRecord {
  var appValue: TeraRevisionStatus {
    get throws
  {
    guard schemaVersion == 1 else { throw TeraGeneratedSubmission.mismatch() }
    return try TeraRevisionStatus(
      operationID: operationId,
      replacement: replacement.appValue,
      retraction: retraction?.appValue,
      policy: policy.appValue,
      phase: phase.appValue,
      original: TeraRevisionTarget(cardID: original.cardId, sourceEventID: original.sourceEventId,
                                   sourceAddress: original.sourceAddress, authorPublicKey: original.authorPublicKey),
      replacementProgress: replacementProgress.appValue,
      retractionProgress: retractionProgress.map { try $0.appValue },
      canResume: canResume, canCancel: canCancel
    )
  }
  }
}

extension FfiRevisionPolicy {
  fileprivate var appValue: TeraRevisionPolicy {
    switch self {
    case .replaceThenRetract: .replaceThenRetract
    case .addressableReplacement: .addressableReplacement
    }
  }
}

extension FfiRevisionPhase {
  fileprivate var appValue: TeraRevisionPhase {
    switch self {
    case .replacementPending: .replacementPending
    case .replacementFailed: .replacementFailed
    case .retractionPending: .retractionPending
    case .complete: .complete
    case .partialEffect: .partialEffect
    case .cancelled: .cancelled
    }
  }
}

extension FfiRevisionBranchRecord {
  var appValue: TeraRevisionBranchStatus {
    get throws {
      try TeraRevisionBranchStatus(stopped: stopped, canResume: canResume, canCancel: canCancel,
                                   targets: targets.map(TeraPublicationTargets.decode))
    }
  }
}
