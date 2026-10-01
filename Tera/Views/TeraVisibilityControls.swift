import SwiftUI

struct TeraAuthorVisibilityMenu: View {
  let author: String
  @EnvironmentObject private var visibility: TeraVisibilityStore
  var body: some View {
    Menu {
      Button("Mute author on this account") { Task { await visibility.change(author: author, to: .muted) } }
      Button("Block author on this account", role: .destructive) { Task { await visibility.change(author: author, to: .blocked) } }
    } label: {
      Label("Author visibility", systemImage: "eye.slash")
    }
    .disabled(visibility.isWorking)
    .accessibilityIdentifier("tera.visibility.author")
  }
}

struct TeraVisibilitySettingsSection: View {
  @EnvironmentObject private var visibility: TeraVisibilityStore
  var body: some View {
    Section {
      if let policy = visibility.policy {
        if policy.entries.isEmpty {
          Text("No authors are hidden on this account.")
        }
        ForEach(policy.entries) { entry in
          VStack(alignment: .leading) {
            Text(entry.author).font(.caption.monospaced()).textSelection(.enabled)
            Text(entry.visibility == .blocked ? "Blocked" : "Muted")
            Button("Restore author to local results") {
              Task { await visibility.change(author: entry.author, to: .visible) }
            }
            .disabled(visibility.isWorking)
          }
        }
      }
      if let message = visibility.message {
        Text(message)
      }
      Button("Reload visibility preferences") { Task { await visibility.load() } }
        .disabled(visibility.isWorking)
    } header: { Text("Hidden authors") }
    footer: { Text("Mute and block hide an author's posts, replies, profile and media across this account's local views. These preferences are not published. They do not remove remote copies or prevent others from posting. Restore is an explicit local action.") }
    .task { await visibility.load() }
    .accessibilityIdentifier("tera.visibility.settings")
  }
}
