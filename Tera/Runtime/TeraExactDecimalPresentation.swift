import Foundation

/// Display admitted wire decimals as strings. NumberFormatter supplies locale
/// symbols and grouping only; it never receives the full monetary value.
struct TeraExactDecimalPresentation {
  var locale: Locale = .current

  func string(_ canonical: String) -> String? {
    // The public food contract admits at most 28 digits plus one decimal dot.
    // This is a bounded presentation guard, not a second publication validator.
    guard canonical.utf8.count <= 29 else { return nil }
    let parts = canonical.split(separator: ".", omittingEmptySubsequences: false)
    guard (1 ... 2).contains(parts.count), parts.allSatisfy({ !$0.isEmpty }),
          parts.allSatisfy({ $0.utf8.allSatisfy { (48 ... 57).contains($0) } }),
          parts.reduce(0, { $0 + $1.utf8.count }) <= 28 else { return nil }
    let format = NumberFormatter()
    format.locale = locale
    format.numberStyle = .decimal
    let glyphs = (0 ... 9).map { format.string(from: NSNumber(value: $0)) ?? String($0) }
    let whole = grouped(parts[0], format: format)
    let localized = whole.utf8.map { byte -> String in
      (48 ... 57).contains(byte) ? glyphs[Int(byte - 48)] : String(UnicodeScalar(byte))
    }.joined()
    // Translate digits before introducing possibly multibyte grouping symbols.
    let integer = localized.replacingOccurrences(of: "|", with: format.groupingSeparator ?? "")
    guard parts.count == 2 else { return integer }
    let fraction = parts[1].utf8.map { glyphs[Int($0 - 48)] }.joined()
    return integer + (format.decimalSeparator ?? ".") + fraction
  }

  private func grouped(_ whole: Substring, format: NumberFormatter) -> String {
    let primary = format.groupingSize
    guard format.usesGroupingSeparator, primary > 0, whole.count > primary else { return String(whole) }
    let secondary = format.secondaryGroupingSize > 0 ? format.secondaryGroupingSize : primary
    var end = whole.endIndex
    var width = primary
    var groups: [Substring] = []
    while end > whole.startIndex {
      let start = whole.index(end, offsetBy: -width, limitedBy: whole.startIndex) ?? whole.startIndex
      groups.append(whole[start ..< end])
      end = start
      width = secondary
    }
    return groups.reversed().joined(separator: "|")
  }
}
