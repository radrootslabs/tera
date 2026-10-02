import SwiftUI

struct TeraAddSaveStatus: View {
  let message: String?
  let symbol: String
  let state: TeraComposerSaveState
  var mediaMessage: String?
  let protection: TeraEditingProtection
  @ObservedObject var repairs: TeraNativeRepairStore
  @Environment(\.dynamicTypeSize) private var dynamicTypeSize

  var body: some View {
    Section {
      Text(state.label)
        .foregroundStyle(.secondary)
        .id("tera.add.save-state")
        .accessibilityIdentifier("tera.add.save-state")
      if let mediaMessage {
        Text(mediaMessage).accessibilityIdentifier("tera.add.media-recovery")
      }
      if let message {
        HStack(alignment: .firstTextBaseline) {
          if !dynamicTypeSize.isAccessibilitySize {
            Image(systemName: symbol).accessibilityHidden(true)
          }
          Text(message)
            .id("tera.add.operation-status")
            .accessibilityIdentifier("radroots.add.status")
        }
          .accessibilityElement(children: .contain)
          .frame(maxWidth: .infinity, alignment: .leading)
          .lineLimit(nil)
          .foregroundStyle(.secondary)
      }
    }
    TeraEditingProtectionActions(protection: protection)
    TeraNativeRepairView(store: repairs)
  }
}
