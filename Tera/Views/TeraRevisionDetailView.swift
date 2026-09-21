import SwiftUI

struct TeraRevisionDetailView: View {
  @ObservedObject var store: TeraRevisionDetailStore
  let operationID: String

  var body: some View {
    List {
      if store.isWorking {
        ProgressView("Loading current revision…")
      }
      if let message = store.message {
        Text(message)
      }
      if let status = store.status {
        Section("Revision") {
          Text(status.honestSummary).accessibilityIdentifier("tera.revision.summary")
          Text("Each relay acts independently. Stopping local work cannot undo accepted or uncertain effects. Retraction is a deletion request, not proof of erasure.")
            .font(.footnote)
          if let original = status.original {
            LabeledContent("Original event", value: original.sourceEventID)
              .textSelection(.enabled)
            if let address = original.sourceAddress {
              LabeledContent("Original address", value: address).textSelection(.enabled)
            }
          }
        }
        branch("Replacement", draft: status.replacement, progress: status.replacementProgress)
        if let child = status.retraction, let progress = status.retractionProgress {
          branch("Retraction request", draft: child, progress: progress)
        } else if status.policy == .replaceThenRetract {
          Section("Retraction request") {
            Text("No retraction child is saved yet. Eligible saved relay evidence is required before it can proceed.")
          }
        }
        Section("Local actions") {
          if status.canResume {
            Button("Resume saved revision") { Task { await store.resume() } }
              .accessibilityIdentifier("tera.revision.resume")
          }
          if status.canCancel {
            Button("Stop pending revision work", role: .destructive) { Task { await store.cancel() } }
              .accessibilityIdentifier("tera.revision.stop")
          }
          if !status.canResume {
            Text("No delivery can resume now. Refresh to check current permissions. Work that needs attention or has been stopped is not retried automatically.")
              .font(.footnote)
          }
        }
      }
      Button("Refresh current details") { Task { await store.load(operationID) } }
    }
    .disabled(store.isWorking)
    .navigationTitle("Revision details")
    .task(id: operationID) { await store.load(operationID) }
    .onDisappear { store.stop() }
    .accessibilityIdentifier("tera.revision.details")
  }

  private func branch(_ title: String, draft: TeraDraftStatus, progress: TeraRevisionBranchStatus) -> some View {
    Section(title) {
      LabeledContent("Saved operation", value: draft.id).textSelection(.enabled)
      Text(draft.settlement?.summary ?? draft.state.label)
      if progress.stopped {
        Text("Further local work is stopped; retained relay evidence remains visible.")
      }
      if let details = progress.targets {
        TeraPublicationTargetsView(details: details)
      } else {
        Text("No frozen relay delivery record is available yet.").font(.footnote)
      }
    }
  }
}
