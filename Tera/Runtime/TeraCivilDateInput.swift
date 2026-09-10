import Foundation

/// Partial native component entry stays separate from a validated civil date.
struct TeraCivilDateInput {
  let raw: String?

  func component(_ index: Int) -> String {
    let parts = raw?.split(separator: "-", omittingEmptySubsequences: false) ?? []
    return parts.indices.contains(index) ? String(parts[index]) : ""
  }

  var value: TeraCivilDate? {
    guard let raw, raw.utf8.count <= 64,
          raw.split(separator: "-", omittingEmptySubsequences: false).count == 3,
          let year = number(component(0), maximumDigits: 4),
          let month = number(component(1), maximumDigits: 2),
          let day = number(component(2), maximumDigits: 2),
          let typedYear = UInt16(exactly: year), let typedMonth = UInt8(exactly: month),
          let typedDay = UInt8(exactly: day)
    else { return nil }
    return try? TeraCivilDate(year: typedYear, month: typedMonth, day: typedDay)
  }

  func replacing(_ index: Int, with input: String) -> String? {
    guard (0 ... 2).contains(index) else { return raw }
    var parts = (0 ... 2).map(component)
    parts[index] = String(input.prefix(index == 0 ? 4 : 2))
    guard parts.contains(where: { !$0.isEmpty }) else { return nil }
    return parts.joined(separator: "-")
  }

  var canonicalOrRaw: String? {
    value?.canonical ?? raw
  }

  private func number(_ text: String, maximumDigits: Int) -> Int? {
    guard !text.isEmpty, text.count <= maximumDigits else { return nil }
    var value = 0
    for character in text {
      guard let digit = character.wholeNumberValue, (0 ... 9).contains(digit) else { return nil }
      value = value * 10 + digit
    }
    return value
  }
}

extension TeraCivilDate {
  var nextDay: Self? {
    if let next = try? Self(year: year, month: month, day: day + 1) {
      return next
    }
    if let next = try? Self(year: year, month: month + 1, day: 1) {
      return next
    }
    return try? Self(year: year + 1, month: 1, day: 1)
  }
}
