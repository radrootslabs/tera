import SwiftUI

struct TeraTimedCalendarField: View {
  let label: String
  let identifier: String
  let timeZone: TimeZone
  @Binding var seconds: UInt64?

  var body: some View {
    if let instant = TeraCalendarEditing.pickerInstant(seconds) {
      DatePicker(label, selection: Binding(
        get: { TeraCalendarEditing.pickerInstant(seconds) ?? instant },
        set: {
          if let value = TeraCalendarEditing.pickerSeconds($0),
             TeraWallTime.from(value, timeZone: timeZone)?.instants(in: timeZone).contains(value) == true
          {
            seconds = value
          }
        }
      ))
      .environment(\.calendar, Calendar(identifier: .gregorian))
      .environment(\.timeZone, timeZone)
      .accessibilityLabel(label)
      .accessibilityIdentifier(identifier)
      repeatedTime
    } else {
      LabeledContent(label) {
        Button(seconds == nil ? "Set time" : "Replace unsupported time") {
          seconds = TeraCalendarEditing.pickerSeconds(Date())
        }
      }
      .accessibilityIdentifier(identifier)
    }
  }

  @ViewBuilder
  private var repeatedTime: some View {
    let values = seconds.flatMap { TeraWallTime.from($0, timeZone: timeZone) }?.instants(in: timeZone) ?? []
    if values.count == 2 {
      Picker("\(label), repeated time", selection: $seconds) {
        ForEach(Array(values.enumerated()), id: \.element) { index, value in
          Text(occurrence(value, index: index)).tag(Optional(value))
        }
      }
      .accessibilityIdentifier("\(identifier).occurrence")
    }
  }

  private func occurrence(_ seconds: UInt64, index: Int) -> String {
    guard let instant = TeraCalendarEditing.pickerInstant(seconds) else { return "Time unavailable" }
    let zone = timeZone.abbreviation(for: instant) ?? timeZone.identifier
    return "\(index == 0 ? "First" : "Second") occurrence (\(zone))"
  }
}
