import Foundation

/// Locale field order and symbols applied directly to Gregorian components.
/// No civil date is represented by an instant, even temporarily.
struct TeraCivilDateFormat {
  let locale: Locale

  func string(_ value: TeraCivilDate) -> String {
    guard let pattern = DateFormatter.dateFormat(fromTemplate: "yMMMd", options: 0, locale: locale),
          pattern.utf8.count <= 128
    else { return value.canonical }
    let symbols = DateFormatter()
    symbols.locale = locale
    symbols.calendar = Calendar(identifier: .gregorian)
    return render(pattern, value: value, symbols: symbols) ?? value.canonical
  }

  private func render(_ pattern: String, value: TeraCivilDate, symbols: DateFormatter) -> String? {
    let characters = Array(pattern)
    var index = 0
    var quoted = false
    var output = ""
    while index < characters.count {
      let character = characters[index]
      if character == "'" {
        if index + 1 < characters.count, characters[index + 1] == "'" {
          output.append("'")
          index += 2
        } else {
          quoted.toggle()
          index += 1
        }
      } else if !quoted, character.isASCII, character.isLetter {
        let start = index
        while index < characters.count, characters[index] == character {
          index += 1
        }
        guard let field = field(character, count: index - start, value: value, symbols: symbols) else { return nil }
        output += field
      } else {
        output.append(character)
        index += 1
      }
    }
    return !quoted && output.utf8.count <= 512 ? output : nil
  }

  private func field(_ field: Character, count: Int, value: TeraCivilDate, symbols: DateFormatter) -> String? {
    switch field {
    case "y": number(Int(value.year), minimumDigits: count == 2 ? 1 : min(count, 4))
    case "d": number(Int(value.day), minimumDigits: min(count, 2))
    case "M", "L": month(Int(value.month), count: count, standalone: field == "L", symbols: symbols)
    case "G": symbols.eraSymbols.last
    default: nil
    }
  }

  private func month(_ month: Int, count: Int, standalone: Bool, symbols: DateFormatter) -> String? {
    if count <= 2 {
      return number(month, minimumDigits: count)
    }
    let names: [String] = switch count {
    case 3: standalone ? symbols.shortStandaloneMonthSymbols : symbols.shortMonthSymbols
    case 4: standalone ? symbols.standaloneMonthSymbols : symbols.monthSymbols
    default: standalone ? symbols.veryShortStandaloneMonthSymbols : symbols.veryShortMonthSymbols
    }
    return names.indices.contains(month - 1) ? names[month - 1] : nil
  }

  private func number(_ value: Int, minimumDigits: Int) -> String? {
    let formatter = NumberFormatter()
    formatter.locale = locale
    formatter.numberStyle = .decimal
    formatter.usesGroupingSeparator = false
    formatter.minimumIntegerDigits = minimumDigits
    formatter.maximumFractionDigits = 0
    return formatter.string(from: NSNumber(value: value))
  }
}

extension TeraCivilDate {
  var previousDay: Self? {
    if day > 1 {
      return try? Self(year: year, month: month, day: day - 1)
    }
    guard year > 1 || month > 1 else { return nil }
    let previousYear = month == 1 ? year - 1 : year
    let previousMonth = month == 1 ? 12 : month - 1
    // At most four validated component candidates; no timezone arithmetic.
    for previousDay: UInt8 in stride(from: 31, through: 28, by: -1) {
      if let value = try? Self(year: previousYear, month: previousMonth, day: previousDay) {
        return value
      }
    }
    return nil
  }
}
