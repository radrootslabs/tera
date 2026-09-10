import SwiftUI

struct TeraAddSaveStatus: View {
  let message: String?
  let symbol: String
  let state: TeraComposerSaveState

  var body: some View {
    Section {
      Text(state.label)
        .foregroundStyle(.secondary)
        .accessibilityIdentifier("tera.add.save-state")
      if let message {
        Label(message, systemImage: symbol)
          .foregroundStyle(.secondary)
          .accessibilityIdentifier("radroots.add.status")
      }
    }
  }
}
