import SwiftUI

struct TeraStorageSettingsSection: View {
  @ObservedObject var settingsStore: TeraSettingsStore
  @ObservedObject var todayStore: TeraTodayStore
  @EnvironmentObject private var appModel: TeraAppModel

  var body: some View {
      Section("Local media storage") {
        Stepper(
          "Cache: \(settingsStore.mediaCacheMegabytes) MB",
          value: $settingsStore.mediaCacheMegabytes,
          in: 16 ... 2048,
          step: 16
        )
        Stepper(
          "Artifacts: \(settingsStore.mediaCacheArtifacts)",
          value: $settingsStore.mediaCacheArtifacts,
          in: 1 ... 10000,
          step: 100
        )
        Button("Free cached photos") {
          guard let context = todayStore.selectedContext else { return }
          Task { await settingsStore.cleanupMediaCache(context: context) }
        }
        .disabled(settingsStore.isWorking || todayStore.selectedContext == nil)
        .accessibilityIdentifier("tera.settings.cache.cleanup")
        Text("Clears cached photos from this context. Run again if more remain. Pending drafts and transfer receipts are retained. If storage is completely full, free device space first.")
          .font(.caption)
          .foregroundStyle(.secondary)
        Button("Save network and storage settings") {
          Task {
            if await settingsStore.saveSettings() {
              await appModel.applySettingsReconfiguration()
            }
          }
        }
        .disabled(settingsStore.isWorking)
        .accessibilityIdentifier("radroots.settings.save")
        if let message = settingsStore.message {
          Text(message).foregroundStyle(.secondary)
        }
        if let failureCode = settingsStore.failureCode {
          Text("Error code \(failureCode)")
            .font(.caption.monospaced())
            .foregroundStyle(.secondary)
            .accessibilityIdentifier("radroots.settings.failure_code")
        }
      }
  }
}
