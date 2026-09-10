import Foundation

extension TeraAddStore {
  var selectedSchema: TeraAddSchema? {
    schemas.first(where: { $0.commandType == form.commandType })
  }

  var isFormEditable: Bool {
    guard activeDraft?.isRevision != true, activeDraft?.kind != .retraction else { return false }
    return activeDraft?.state.isEditable ?? true
  }

  var isProductReady: Bool {
    state == .ready && selectedSchema != nil
  }

  var canSave: Bool {
    isProductReady && isFormEditable && !isWorking
  }

  var canSubmit: Bool {
    isProductReady && !isWorking
      && (activeDraft?.isRevision == true || activeDraft?.state.canAdvance == true
        || isFormEditable)
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
