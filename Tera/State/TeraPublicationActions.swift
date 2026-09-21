import Foundation

extension TeraSubmissionStore {
  var canContinue: Bool {
    hasAction && !isWorking && status?.canOfferContinuation != false
  }

  var actionLabel: String {
    if isWorking {
      return "Submitting captured form…"
    }
    if status?.canOfferContinuation != false {
      return "Continue original submission"
    }
    switch status?.retry {
    case .complete: return "Delivery policy complete"
    case .stopped: return "Publication stopped"
    case .deferredUntil, .inFlightUntil: return "Publication waiting"
    case .needsAction: return "Publication needs attention"
    default: return "Review saved submission"
    }
  }

  var actionExplanation: String {
    [status?.summary, status?.retry.explanation, message]
      .compactMap(\.self).joined(separator: " ")
  }
}

extension TeraAddStore {
  var submitAccessibilityValue: String {
    if activeDraft == nil, submissions.hasAction {
      let message = submissions.actionExplanation
      if let code = submissions.failureCode {
        return "\(message) Error code \(code)"
      }
      return message
    }
    if isWorking {
      return "Working"
    }
    if let message {
      if let code = lastFailureCode {
        return "\(message) Error code \(code)"
      }
      return message
    }
    if let activeDraft {
      return activeDraft.honestSummary
    }
    return canSubmit ? "Ready" : "Unavailable"
  }

  var submitLabel: String {
    if activeDraft == nil, submissions.hasAction {
      return submissions.actionLabel
    }
    if activeDraft?.kind == .retraction {
      return "Retry retraction"
    }
    if activeDraft?.coordinateWritable == false {
      return "Publication held"
    }
    if activeDraft?.canAdvance == true {
      return "Retry delivery"
    }
    return "Submit"
  }
}
