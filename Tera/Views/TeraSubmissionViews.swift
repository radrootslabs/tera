import SwiftUI

struct TeraSubmissionStatusView: View {
  @ObservedObject var store: TeraSubmissionStore

  var body: some View {
    if store.hasAction {
      Section("Submission") {
        if store.status?.delivery.isStopped != true {
          Button("Stop publication") { Task { await store.requestStop() } }
            .accessibilityIdentifier("tera.add.submission.stop")
        }
        if store.isWorking {
          ProgressView(store.status?.summary ?? "Checking the original submission request…")
            .accessibilityIdentifier("tera.add.submission.progress")
          Button("Stop waiting") { store.stopWaiting() }
            .accessibilityIdentifier("tera.add.submission.stop_waiting")
        }
        if !store.isWorking {
          if store.request != nil {
            Button("Check saved submission status") { Task { await store.refreshSelected() } }
              .accessibilityIdentifier("tera.add.submission.check")
          }
          if store.status?.canOfferContinuation != false {
            Button("Continue original submission") { Task { await store.continueSelected() } }
              .accessibilityIdentifier("tera.add.submission.continue")
          }
        }
        if let status = store.status {
          Text(status.summary)
            .accessibilityIdentifier("tera.add.submission.status")
          TeraPublicationTargetsView(details: status.targetDetails)
          if !status.media.isEmpty {
            Text(status.mediaSummary)
              .accessibilityIdentifier("tera.add.submission.media_status")
          }
          DisclosureGroup("Captured form") {
            Text(status.captured.form.editingValue.commandType.label).font(.headline)
            Text(status.captured.form.editingValue.content)
            if let title = status.captured.form.editingValue.title {
              Text(title)
            }
            Text("Editing continues separately. This submission keeps the form captured when you tapped Submit.")
              .font(.footnote)
          }
          .accessibilityIdentifier("tera.add.submission.captured")
        }
        if let message = store.message {
          Text(message).accessibilityIdentifier("tera.add.submission.message")
        }
        Text("Retry uses this original request. Choose New for another intentional submission.")
          .font(.footnote)
      }
    }
  }
}

struct TeraSubmissionInventoryView: View {
  @ObservedObject var store: TeraSubmissionStore
  @ObservedObject private var inventory: TeraSubmissionInventory

  init(store: TeraSubmissionStore) {
    self.store = store
    inventory = store.inventory
  }

  var body: some View {
    Section("Submissions") {
      if inventory.isLoading {
        ProgressView("Loading submissions…")
      }
      if let message = inventory.message {
        Text(message).foregroundStyle(.secondary)
      }
      ForEach(inventory.entries) { entry in
        switch entry {
        case let .submission(summary):
          VStack(alignment: .leading, spacing: 8) {
            Text(label(summary.state))
            Text(Date(timeIntervalSince1970: Double(summary.reservedAtUnixMilliseconds) / 1000),
                 format: .dateTime.year().month().day().hour().minute()).font(.caption)
            Button("View submission") { Task { await store.select(summary) } }
              .disabled(store.isWorking)
          }
          .accessibilityIdentifier("tera.add.submission.\(summary.id)")
        case let .repair(_, _, reason):
          Text(reason == .unsupportedSchema
            ? "This saved submission needs a compatible app. Its data is preserved."
            : "This submission needs attention. Its data is preserved; other work remains available.")
            .accessibilityIdentifier("tera.add.submission.repair")
        }
      }
      if inventory.cursor != nil {
        Button("Next submissions page") { inventory.more() }.disabled(inventory.isLoading)
      }
      Button("Reload submissions") { inventory.start() }.disabled(inventory.isLoading)
    }
    TeraSubmissionStatusView(store: store)
  }

  private func label(_ state: TeraSubmissionSummaryState) -> String {
    switch state {
    case .reserved: "Reserved; local preparation is pending."
    case let .operation(_, _, _, state): state.label
    }
  }
}
