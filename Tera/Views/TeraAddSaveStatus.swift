import SwiftUI

struct TeraAddSaveStatus: View {
  let message: String?
  let symbol: String
  let state: TeraComposerSaveState
  var mediaMessage: String?
  let protection: TeraEditingProtection

  var body: some View {
    Section {
      Text(state.label)
        .foregroundStyle(.secondary)
        .accessibilityIdentifier("tera.add.save-state")
      if let mediaMessage {
        Text(mediaMessage).accessibilityIdentifier("tera.add.media-recovery")
      }
      if let message {
        Label(message, systemImage: symbol)
          .foregroundStyle(.secondary)
          .accessibilityIdentifier("radroots.add.status")
      }
    }
    TeraEditingProtectionActions(protection: protection)
  }
}
