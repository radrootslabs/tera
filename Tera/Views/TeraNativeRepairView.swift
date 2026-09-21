import SwiftUI

struct TeraNativeRepairView: View {
  @ObservedObject var store: TeraNativeRepairStore

  var body: some View {
    if store.message != nil || store.isRunning || !store.issues.isEmpty {
      Section("Saved photo recovery") {
        TeraNativeRepairProgressView(message: store.message, remaining: store.progress?.remaining)
        if store.issues.count == TeraNativeRepairStore.previewLimit {
          Text("Showing up to \(TeraNativeRepairStore.previewLimit) repair notices.")
        }
        ForEach(Array(store.issues.enumerated()), id: \.element.id) { index, issue in
          TeraNativeRepairActions(issue: issue, index: index, isRunning: store.isRunning) { store.check(issue) }
        }
        if store.isRunning {
          ProgressView("Checking saved photos")
        } else {
          Button("Check saved photos again") { store.retry() }
            .accessibilityIdentifier("tera.add.recovery.check")
        }
      }
      .accessibilityIdentifier("tera.add.recovery.issues")
    }
  }
}

struct TeraNativeRepairProgressView: View {
  let message: String?
  let remaining: Int?

  var body: some View {
    if let message {
      Text(message).accessibilityIdentifier("tera.add.recovery.message")
    }
    if let remaining, remaining > 0 {
      Text("\(remaining) more saved transfer records remain to be checked in this pass.")
        .accessibilityIdentifier("tera.add.recovery.remaining")
    }
  }
}

struct TeraNativeRepairActions: View {
  let issue: TeraNativeRecoveryIssue
  let index: Int
  let isRunning: Bool
  let check: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text("Saved photo \(index + 1)").font(.headline)
      Text(issue.reason.message)
        .accessibilityIdentifier("tera.add.recovery.reason.\(issue.key)")
      if issue.status == nil {
        Text("This recovery status could not be saved. Check again when local storage is available.")
          .foregroundStyle(.secondary)
      }
      Button("Check this photo’s saved evidence", action: check)
        .disabled(isRunning)
        .buttonStyle(.borderless)
        .accessibilityIdentifier("tera.add.recovery.check.\(issue.key)")
      Text("Checking keeps the evidence and does not start another upload. A missing or mismatched request must be restored before recovery can succeed.")
        .font(.footnote)
    }
    .accessibilityElement(children: .contain)
  }
}

extension TeraNativeRecoveryReason {
  var message: String {
    switch self {
    case .missingParent: "The saved request for this photo could not be found. Its upload evidence has been kept."
    case .invalidParent: "The saved request for this photo needs repair. Its upload evidence has been kept."
    case .associationMismatch: "This upload does not match its saved request. Its evidence has been kept."
    case .outcomeUnconfirmed: "This photo’s upload result is not yet confirmed. Its evidence has been kept."
    case .resolved: "This photo has been recovered."
    }
  }
}
