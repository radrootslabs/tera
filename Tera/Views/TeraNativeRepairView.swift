import SwiftUI

struct TeraNativeRepairView: View {
  @ObservedObject var store: TeraNativeRepairStore

  var body: some View {
    if store.message != nil || store.isRunning || !store.issues.isEmpty {
      Section("Saved photo recovery") {
        if let message = store.message {
          Text(message)
        }
        if let remaining = store.progress?.remaining, remaining > 0 {
          Text("\(remaining) more saved photos remain to be checked.")
        }
        if store.issues.count == TeraNativeRepairStore.previewLimit {
          Text("Showing up to \(TeraNativeRepairStore.previewLimit) repair notices.")
        }
        ForEach(Array(store.issues.enumerated()), id: \.element.id) { index, issue in
          VStack(alignment: .leading) {
            Text("Saved photo \(index + 1)").font(.headline)
            Text(issue.reason.message)
            if issue.status == nil {
              Text("This recovery status could not be saved. Check again when local storage is available.")
                .foregroundStyle(.secondary)
            }
          }
          .accessibilityElement(children: .combine)
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

private extension TeraNativeRecoveryReason {
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
