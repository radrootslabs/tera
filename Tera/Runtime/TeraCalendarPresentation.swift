import Foundation

struct TeraCalendarPresentation {
  var locale: Locale = .current
  var timeZone: TimeZone = .current

  func summary(_ timing: TeraCalendarTiming) -> String {
    switch timing {
    case let .dateBased(start, end):
      let format = TeraCivilDateFormat(locale: locale)
      guard let end else { return "All day, \(format.string(start))" }
      guard end > start, let last = end.previousDay else { return "Date unavailable" }
      let range = last == start ? format.string(start) : "\(format.string(start)) – \(format.string(last))"
      return "All day, \(range)"
    case let .timeBased(start, end, startZone, endZone):
      guard end == nil || end.map({ $0 > start }) == true else { return "Date unavailable" }
      var parts = [instant(start, zone: timeZone, label: "Starts")]
      if let end {
        parts.append(instant(end, zone: timeZone, label: "Ends"))
      }
      parts.append("Display time zone: \(timeZone.identifier)")
      if let startZone {
        parts.append(sourceInstant(start, zoneID: startZone, label: "Event start"))
      }
      if let end, let endZone {
        parts.append(sourceInstant(end, zoneID: endZone, label: "Event end"))
      }
      return parts.joined(separator: "; ")
    }
  }

  private func sourceInstant(_ seconds: UInt64, zoneID: String, label: String) -> String {
    guard zoneID.utf8.count <= 255, let zone = TimeZone(identifier: zoneID) else { return "Date unavailable" }
    return "\(instant(seconds, zone: zone, label: label)) (\(zoneID))"
  }

  private func instant(_ seconds: UInt64, zone: TimeZone, label: String) -> String {
    guard let date = TeraCalendarTiming.presentationInstant(seconds) else { return "Date unavailable" }
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = zone
    guard (1 ... 9999).contains(calendar.component(.year, from: date)) else { return "Date unavailable" }
    let format = DateFormatter()
    format.locale = locale
    format.calendar = calendar
    format.timeZone = zone
    format.dateStyle = .medium
    format.timeStyle = .long
    return "\(label): \(format.string(from: date))"
  }
}

extension TeraTodayCard {
  var accessibilitySummary: String {
    accessibilitySummary(locale: .current, timeZone: .current)
  }

  func accessibilitySummary(locale: Locale, timeZone: TimeZone) -> String {
    var parts = [type.label, "by \(authorName)"]
    if let title {
      parts.append(title)
    }
    if !content.isEmpty {
      parts.append(content)
    }
    if let calendarTiming {
      parts.append(TeraCalendarPresentation(locale: locale, timeZone: timeZone).summary(calendarTiming))
    }
    if let priceSummary {
      parts.append(priceSummary)
    }
    if lifecycle != .active {
      parts.append(lifecycle.rawValue)
    }
    if let localOperationState {
      parts.append(localOperationState)
    }
    return parts.joined(separator: ", ")
  }
}
