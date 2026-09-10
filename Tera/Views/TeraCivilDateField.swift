import SwiftUI

struct TeraCivilDateField: View {
  let label: String
  let identifier: String
  @Binding var raw: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text(label).font(.subheadline.weight(.semibold))
      ViewThatFits(in: .horizontal) {
        HStack { components }
        VStack(alignment: .leading) { components }
      }
      if raw != nil, TeraCivilDateInput(raw: raw).value == nil {
        Text("Enter a valid year, month and day.").font(.caption).foregroundStyle(.secondary)
      }
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(label)
    .accessibilityIdentifier(identifier)
  }

  private var components: some View {
    ForEach(Array(["Year", "Month", "Day"].enumerated()), id: \.offset) { index, name in
      VStack(alignment: .leading) {
        Text(name).font(.caption)
        TextField(name, text: Binding(
          get: { TeraCivilDateInput(raw: raw).component(index) },
          set: { raw = TeraCivilDateInput(raw: raw).replacing(index, with: $0) }
        ))
        .keyboardType(.numberPad)
        .textFieldStyle(.roundedBorder)
        .accessibilityLabel("\(label), \(name)")
        .accessibilityIdentifier("\(identifier).\(name.lowercased())")
      }
      .frame(minWidth: index == 0 ? 90 : 70)
    }
  }
}
