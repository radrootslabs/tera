import SwiftUI

struct TeraCalendarComposerFields: View {
  @ObservedObject var store: TeraAddStore

  var body: some View {
    Picker("When", selection: binding(\.eventTiming)) {
      ForEach(TeraEventTiming.allCases) { timing in
        Text(timing.label).tag(Optional(timing))
      }
    }
    .accessibilityIdentifier("radroots.add.event_timing")
    if store.form.eventTiming == .allDay {
      TeraCivilDateField(label: "Starts", identifier: "radroots.add.event.start", raw: binding(\.eventStartDate))
      TeraCivilDateField(label: "Ends before (optional)", identifier: "radroots.add.event.end", raw: binding(\.eventEndDate))
      Text("The end date is not included. Leave it empty for a one-day event.")
        .font(.caption).foregroundStyle(.secondary)
    } else {
      timedFields
    }
  }

  @ViewBuilder
  private var timedFields: some View {
    TextField("Event time zone (optional)", text: Binding(
      get: { store.form.eventTimezone ?? "" },
      set: { store.updateForm(\.eventTimezone, $0.isEmpty ? nil : String($0.prefix(255))) }
    ))
    .textInputAutocapitalization(.never).autocorrectionDisabled()
    .accessibilityIdentifier("tera.add.event.timezone")
    if let zone = selectedTimeZone {
      Text(store.form.eventTimezone == nil ? "Device display time: \(zone.identifier)" : "Event time: \(zone.identifier)")
        .font(.caption).foregroundStyle(.secondary)
      TeraTimedCalendarField(label: "Starts", identifier: "radroots.add.event.start", timeZone: zone,
                             seconds: binding(\.eventStartUnixSeconds))
      TeraTimedCalendarField(label: "Ends", identifier: "radroots.add.event.end", timeZone: zone,
                             seconds: binding(\.eventEndUnixSeconds))
      if store.form.eventEndUnixSeconds != nil {
        Button("Remove end time") { store.updateForm(\.eventEndUnixSeconds, nil) }
      }
    } else {
      Text("Choose a supported time zone, such as America/Vancouver.").foregroundStyle(.secondary)
    }
  }

  private var selectedTimeZone: TimeZone? {
    guard let identifier = store.form.eventTimezone else { return .current }
    guard identifier.utf8.count <= 255 else { return nil }
    return TimeZone(identifier: identifier)
  }

  private func binding<Value>(_ keyPath: WritableKeyPath<TeraAddForm, Value>) -> Binding<Value> {
    Binding(get: { store.form[keyPath: keyPath] }, set: { store.updateForm(keyPath, $0) })
  }
}
