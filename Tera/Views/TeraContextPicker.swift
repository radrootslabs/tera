import SwiftUI

struct TeraContextPicker: View {
  @ObservedObject var store: TeraTodayStore
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      List(store.contexts) { context in
        Button {
          store.selectContext(id: context.id)
          dismiss()
        } label: {
          HStack {
            VStack(alignment: .leading, spacing: 4) {
              Text(context.label)
                .foregroundStyle(.primary)
              if let locality = context.locality {
                Text(locality)
                  .font(.caption)
                  .foregroundStyle(.secondary)
              }
            }
            Spacer()
            if context.id == store.selectedContextID {
              Image(systemName: "checkmark")
                .accessibilityHidden(true)
            }
          }
        }
        .accessibilityLabel(context.label)
        .accessibilityValue(context.id == store.selectedContextID ? "Selected" : "")
        .accessibilityIdentifier("radroots.context.\(context.id)")
      }
      .navigationTitle("Local network")
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button("Done") { dismiss() }
        }
      }
    }
    .presentationDetents([.medium, .large])
    .accessibilityIdentifier("radroots.context.picker")
  }
}
