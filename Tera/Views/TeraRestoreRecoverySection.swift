import SwiftUI

struct TeraRestoreRecoverySection: View {
  @ObservedObject var store: TeraRestoreRecoveryStore
  @State private var confirmResume = false

  var body: some View {
    Group {
      if let status = store.status, status.phase != .resumed {
        Section("Restored work is paused") {
          Text("You can use local drafts while saved publications are checked. Nothing resumes just because a backup was restored.")
          Text("\(status.targets.filter { $0.observation == .observed }.count) destination checks found the original publication.")
          Text("\(status.targets.filter { $0.observation == .notObserved }.count) checks did not find it. This does not prove it was never received.")
          if status.targets.contains(where: { $0.observation == nil || $0.observation == .incomplete }) {
            Button("Check next destination") { Task { await store.checkNext() } }
          }
          Button("Review saved work") { Task { await store.review() } }
          if store.reviewedInventory != nil {
            Button("Resume original saved work…") { confirmResume = true }
          }
        }
        .disabled(store.isWorking)
        .accessibilityIdentifier("tera.restore.review")
      }
      if let message = store.message {
        Section("Restore recovery") {
          Text(message)
          Button("Check recovery status") { Task { await store.load() } }.disabled(store.isWorking)
        }
      }
    }
    .task { await store.load() }
    .confirmationDialog("Resume original saved work?", isPresented: $confirmResume, titleVisibility: .visible) {
      Button("Resume original saved work") { Task { await store.resume() } }
      Button("Keep paused", role: .cancel) {}
    } message: {
      Text("Destinations may already have received this work. Resuming keeps the original publication identities and saved destination policy.")
    }
  }
}
