import SwiftUI

struct TeraAddSubmitButton: View {
  @ObservedObject var store: TeraAddStore

  var body: some View {
    Button {
      Task { await store.submit() }
    } label: {
      Text(submitLabel)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity)
    }
    .accessibilityIdentifier("radroots.add.submit")
    .accessibilityValue(submitAccessibilityValue)
    .buttonStyle(.borderedProminent)
    .tint(.primary)
    .disabled(!store.canSubmit)
  }

  private var submitAccessibilityValue: String {
    if store.activeDraft == nil, store.submissions.hasAction {
      let message = store.submissions.message ?? store.submissions.status?.summary ?? "Original submission in progress"
      if let code = store.submissions.failureCode {
        return "\(message) Error code \(code)"
      }
      return message
    }
    if store.isWorking {
      return "Working"
    }
    if let message = store.message {
      if let code = store.lastFailureCode {
        return "\(message) Error code \(code)"
      }
      return message
    }
    if let activeDraft = store.activeDraft {
      return activeDraft.honestSummary
    }
    return store.canSubmit ? "Ready" : "Unavailable"
  }

  private var submitLabel: String {
    if store.activeDraft == nil, store.submissions.hasAction {
      return store.submissions.isWorking ? "Submitting captured form…" : "Retry original submission"
    }
    if store.activeDraft?.kind == .retraction {
      return "Retry retraction"
    }
    if store.activeDraft?.coordinateWritable == false {
      return "Publication held"
    }
    if store.activeDraft?.canAdvance == true {
      return "Retry delivery"
    }
    return "Submit"
  }
}
