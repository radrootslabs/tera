import Foundation

enum TeraCalendarEditing {
  static func civilDate(at seconds: UInt64, timeZone: TimeZone) -> TeraCivilDate? {
    guard let instant = TeraCalendarTiming.presentationInstant(seconds) else { return nil }
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = timeZone
    let parts = calendar.dateComponents([.year, .month, .day], from: instant)
    guard let year = parts.year.flatMap(UInt16.init(exactly:)),
          let month = parts.month.flatMap(UInt8.init(exactly:)),
          let day = parts.day.flatMap(UInt8.init(exactly:))
    else { return nil }
    return try? TeraCivilDate(year: year, month: month, day: day)
  }

  static func initialize(_ form: inout TeraAddForm, now: UInt64, timeZone: TimeZone) {
    form.eventTimezone = timeZone.identifier
    let civil = civilDate(at: now, timeZone: timeZone)
    form.eventStartDate = civil?.canonical
    form.eventEndDate = civil?.nextDay?.canonical
    let (start, startOverflow) = now.addingReportingOverflow(3600)
    let (end, endOverflow) = start.addingReportingOverflow(3600)
    if !startOverflow, TeraCalendarTiming.presentationInstant(start) != nil {
      form.eventStartUnixSeconds = start
    }
    if !startOverflow, !endOverflow, TeraCalendarTiming.presentationInstant(end) != nil {
      form.eventEndUnixSeconds = end
    }
  }

  static func pickerInstant(_ value: UInt64?) -> Date? {
    guard let value, value > 0 else { return nil }
    return TeraCalendarTiming.presentationInstant(value)
  }

  static func pickerSeconds(_ value: Date) -> UInt64? {
    guard let seconds = try? TeraClock.unixSeconds(from: value, requirePositive: true),
          TeraCalendarTiming.presentationInstant(seconds) != nil
    else { return nil }
    return seconds
  }
}

/// A wall time exists only in conjunction with an explicit zone. Strict matching
/// returns no instant for a gap and both distinct instants for an overlap.
struct TeraWallTime {
  let date: TeraCivilDate
  var hour: Int
  var minute: Int
  var second: Int = 0

  func instants(in timeZone: TimeZone) -> [UInt64] {
    guard (0 ... 23).contains(hour), (0 ... 59).contains(minute), (0 ... 59).contains(second) else { return [] }
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = timeZone
    let components = DateComponents(year: Int(date.year), month: Int(date.month), day: Int(date.day),
                                    hour: hour, minute: minute, second: second)
    guard let noon = calendar.date(from: DateComponents(year: Int(date.year), month: Int(date.month),
                                                        day: Int(date.day), hour: 12)),
          let anchor = calendar.date(byAdding: .day, value: -1, to: noon)
    else { return [] }
    let candidates = [Calendar.RepeatedTimePolicy.first, .last].compactMap { policy -> UInt64? in
      guard let found = calendar.nextDate(after: anchor, matching: components, matchingPolicy: .strict,
                                          repeatedTimePolicy: policy, direction: .forward),
            matches(found, components: components, calendar: calendar)
      else { return nil }
      return TeraCalendarEditing.pickerSeconds(found)
    }
    return Array(Set(candidates)).sorted()
  }

  private func matches(_ instant: Date, components: DateComponents, calendar: Calendar) -> Bool {
    let fields: Set<Calendar.Component> = [.year, .month, .day, .hour, .minute, .second]
    let actual = calendar.dateComponents(fields, from: instant)
    return fields.allSatisfy { actual.value(for: $0) == components.value(for: $0) }
  }

  static func from(_ seconds: UInt64, timeZone: TimeZone) -> Self? {
    guard let instant = TeraCalendarEditing.pickerInstant(seconds),
          let date = TeraCalendarEditing.civilDate(at: seconds, timeZone: timeZone)
    else { return nil }
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = timeZone
    return Self(date: date, hour: calendar.component(.hour, from: instant),
                minute: calendar.component(.minute, from: instant), second: calendar.component(.second, from: instant))
  }
}
