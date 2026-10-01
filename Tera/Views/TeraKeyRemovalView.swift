import SwiftUI

enum TeraKeyRemovalConsent {
  static let local = "This removes only this device’s signing key. Drafts, photos, settings, signed requests and receipts remain on this device. This does not erase local data."
  static let remote = "Published copies may remain with relays and other people. Key removal does not retract them. To request retraction, use Retract on each post first, then select its saved request below. Linked revisions must finish in Drafts & outbox."
  static let recovery = "Selected requests are signed and saved before removal; delivery is not guaranteed. The app stops before removing the key. To open this account’s retained data or resume delivery later, you must import the same key from your own backup. Unselected unsigned work cannot be signed without that key."
}

struct TeraKeyRemovalView: View {
  let author: String
  @EnvironmentObject private var appModel: TeraAppModel
  @Environment(\.dismiss) private var dismiss
  @State private var requests: [TeraLegacyDraftSummary] = []
  @State private var selected: Set<String> = []
  @State private var cursor: String?
  @State private var loaded = false
  @State private var loading = false
  @State private var consent = false
  @State private var message: String?
  @State private var removal: Task<Void, Never>?

  var body: some View {
    NavigationStack {
      List {
        Section("Local signing key") {
          Text(TeraKeyRemovalConsent.local)
          Text(author).font(.caption.monospaced())
        }
        Section("Remote deletion requests") {
          Text(TeraKeyRemovalConsent.remote)
          ForEach(requests) { request in
            Toggle(isOn: Binding(get: { selected.contains(request.id) }, set: { enabled in
              if enabled {
                selected.insert(request.id)
              } else {
                selected.remove(request.id)
              }
            })) {
              VStack(alignment: .leading) {
                Text("Saved retraction \(request.id.prefix(8))").font(.caption.monospaced())
                Text(request.honestSummary).font(.caption)
              }
            }
          }
          if loaded, requests.isEmpty {
            Text("No independent saved retraction requests are shown.")
          }
          if cursor != nil {
            Button("Next page of requests") { Task { await load() } }
              .disabled(!selected.isEmpty || loading)
            Text("Selections apply to this page only. Prepare other requests in Drafts & outbox before removal.").font(.caption)
          }
          if loading {
            ProgressView()
          }
          if let message {
            Text(message)
          }
        }
        Section("Before removal") {
          Text(TeraKeyRemovalConsent.recovery)
          Toggle("I understand the retained data and loss of signing access", isOn: $consent)
            .accessibilityIdentifier("tera.keyRemoval.consent")
          Button("Remove this signing key", role: .destructive) {
            let chosen = requests.filter { selected.contains($0.id) }.map {
              TeraKeyRemovalRequest(id: $0.id, revision: $0.revision)
            }
            removal = Task {
              await appModel.removeSigningKey(author: author, requests: chosen)
              removal = nil
              dismiss()
            }
          }
          .disabled(!consent || !loaded || loading || removal != nil)
          .accessibilityIdentifier("tera.keyRemoval.confirm")
        }
      }
      .disabled(removal != nil)
      .navigationTitle("Remove signing key")
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Cancel") { removal?.cancel(); dismiss() }
        }
      }
      .task {
        if !loaded {
          await load()
        }
      }
      .onDisappear { removal?.cancel() }
      .interactiveDismissDisabled(removal != nil)
    }
  }

  private func load() async {
    guard !loading else { return }
    loading = true
    defer { loading = false }
    do {
      let page = try await appModel.keyRemovalPage(author: author, cursor: cursor)
      try Task.checkCancellation()
      requests = page.entries.compactMap { entry in
        guard case let .draft(draft) = entry, draft.kind == .retraction,
              draft.revisionParentID == nil else { return nil }
        return draft
      }
      selected.removeAll()
      cursor = page.nextCursor
      loaded = true
      message = nil
    } catch {
      loaded = false
      message = "Saved requests could not be checked. Cancel and retry before removing the key."
    }
  }
}
