import SwiftUI

struct TeraDraftsSheet: View {
  @ObservedObject var store: TeraAddStore
  @ObservedObject private var recovery: TeraDraftRecoveryStore
  @ObservedObject private var protection: TeraEditingProtection
  @Environment(\.dismiss) private var dismiss

  init(store: TeraAddStore) {
    self.store = store
    recovery = store.recovery
    protection = store.protection
  }

  var body: some View {
    NavigationStack {
      List {
        if recovery.isLoading {
          ProgressView("Loading saved work…")
        }
        if let message = store.message {
          Text(message)
        }
        TeraEditingProtectionActions(protection: protection)
        composerSection
        legacySection
        Section {
          Button("Reload from first page") { recovery.start() }
            .disabled(recovery.isLoading)
          Text("Each page shows saved work on this device. Reload to include newly saved drafts.")
            .font(.footnote)
        }
      }
      .navigationTitle("Drafts & outbox")
      .toolbar {
        ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
      }
    }
    .accessibilityIdentifier("radroots.add.drafts.sheet")
    .onAppear { recovery.start() }
    .onChange(of: protection.reopened) { _, value in
      if value != nil {
        dismiss()
      }
    }
  }

  private var composerSection: some View {
    Section("Saved editing") {
      if let error = recovery.composerError {
        Text(error).foregroundStyle(.secondary)
      }
      if recovery.composers.isEmpty, recovery.composerError == nil, !recovery.isLoading {
        Text("No saved editing on this page.").foregroundStyle(.secondary)
      }
      ForEach(Array(recovery.composers.enumerated()), id: \.offset) { _, entry in
        switch entry {
        case let .draft(summary):
          VStack(alignment: .leading) {
            Text(summary.commandType.label).font(.headline)
            savedTime(summary.updatedAtUnixMilliseconds)
            Button("Reopen") { reopen(.composer(summary.id)) }
              .disabled(store.isWorking)
          }
          .accessibilityIdentifier("tera.add.composer.\(summary.id)")
        case let .repair(_, _, reason):
          Text(reason == .unsupportedSchema
            ? "This saved draft needs a compatible app. Its data has been preserved."
            : "This saved draft cannot be read. Its data has been preserved; other drafts remain available.")
            .accessibilityIdentifier("tera.add.composer.repair")
        }
      }
      if recovery.composerCursor != nil {
        Button("Next editing page") { recovery.moreComposers() }
          .disabled(recovery.isLoading)
      }
    }
  }

  private var legacySection: some View {
    Section("Saved operations") {
      if let error = recovery.legacyError {
        Text(error).foregroundStyle(.secondary)
      }
      if recovery.legacy.isEmpty, recovery.legacyError == nil, !recovery.isLoading {
        Text("No saved operations on this page.").foregroundStyle(.secondary)
      }
      ForEach(Array(recovery.legacy.enumerated()), id: \.offset) { _, entry in
        switch entry {
        case let .draft(summary): legacyRow(summary)
        case let .repair(_, _, reason):
          Text(legacyRepairMessage(reason)).accessibilityIdentifier("tera.add.operation.repair")
        }
      }
      if recovery.legacyCursor != nil {
        Button("Next operations page") { recovery.moreLegacy() }
          .disabled(recovery.isLoading)
      }
    }
  }

  private func legacyRow(_ summary: TeraLegacyDraftSummary) -> some View {
    VStack(alignment: .leading, spacing: 8) {
      Text(summary.commandType.label).font(.headline)
      Text(summary.honestSummary).font(.subheadline)
      if summary.mediaCount > 0 {
        Text(summary.mediaSummary).font(.caption).foregroundStyle(.secondary)
          .accessibilityIdentifier("radroots.add.draft.media_status.\(summary.id)")
      }
      savedTime(summary.updatedAtUnixMilliseconds)
      HStack {
        if summary.hasForm {
          Button(summary.state.isEditable ? "Reopen" : "View") { reopen(.legacy(summary.id)) }
        }
        if summary.state.canAdvance {
          Button("Retry") { Task { await store.retry(id: summary.id); recovery.start() } }
        }
        if summary.state.canCancel {
          Button("Cancel", role: .destructive) { Task { await store.cancel(id: summary.id); recovery.start() } }
        }
      }
      .buttonStyle(.borderless)
      .disabled(store.isWorking)
    }
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("radroots.add.draft.\(summary.id)")
  }

  private func savedTime(_ milliseconds: UInt64) -> some View {
    Text(Date(timeIntervalSince1970: Double(milliseconds) / 1000), format: .dateTime.year().month().day().hour().minute())
      .font(.caption).foregroundStyle(.secondary)
  }

  private func reopen(_ selection: TeraDraftRecoverySelection) {
    Task {
      if await store.reopenSaved(selection) {
        dismiss()
      }
    }
  }

  private func legacyRepairMessage(_ reason: TeraLegacyDraftRepairReason) -> String {
    switch reason {
    case .unsupportedSchema: "This saved operation needs a compatible app. Its data has been preserved."
    case .corruptRecord: "This saved operation cannot be read. Its data has been preserved; other operations remain available."
    case .needsAttention: "Local operation details are unavailable. Retry the list; other drafts remain available."
    }
  }
}
