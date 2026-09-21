import SwiftUI

struct TeraAddSubmitButton: View {
  @ObservedObject var store: TeraAddStore

  var body: some View {
    Button {
      Task { await store.submit() }
    } label: {
      Text(store.submitLabel)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity)
    }
    .accessibilityIdentifier("radroots.add.submit")
    .accessibilityValue(store.submitAccessibilityValue)
    .buttonStyle(.borderedProminent)
    .tint(.primary)
    .disabled(!store.canSubmit)
  }
}
