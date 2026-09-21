import Foundation

extension TeraAddStore {
  func selectType(_ type: TeraAddCommandType) {
    guard !isWorking, !submissions.isWorking, isFormEditable, form.commandType != type else { return }
    newDraft(type: type)
  }

  var selectedSchema: TeraAddSchema? {
    schemas.first(where: { $0.commandType == form.commandType })
  }

  var isFormEditable: Bool {
    guard !revisionPreparation.isCaptured, activeDraft?.isRevision != true, activeDraft?.kind != .retraction else { return false }
    return activeDraft?.isEditable ?? true
  }

  var isProductReady: Bool {
    state == .ready && selectedSchema != nil
  }

  var canSave: Bool {
    isProductReady && (isFormEditable || revisionPreparation.isCaptured) && !isWorking
  }

  var canSubmit: Bool {
    isProductReady && !isWorking && !submissions.isWorking
      && activeDraft?.coordinateWritable != false
      && (activeDraft?.isRevision == true || activeDraft?.canAdvance == true
        || activeDraft?.canQueue == true
        || isFormEditable || revisionPreparation.isCaptured)
  }

  var acceptsMedia: Bool {
    mediaLimit > 0
  }

  var canAddMedia: Bool {
    isFormEditable && acceptsMedia && form.media.count < mediaLimit
  }

  var mediaLimit: Int {
    guard
      let maximum = selectedSchema?.fields
        .first(where: { $0.kind == .media })?.maxItems
    else { return 0 }
    return Int(maximum)
  }
}
