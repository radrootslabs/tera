import Foundation

/// Native entry translation only. Shared Rust constructors still validate the
/// canonical amount, digit budget, price/quantity rules, currency and unit.
enum TeraFoodDecimalEntry {
  static func canonicalOrRaw(_ raw: String?, locale: Locale) -> String? {
    guard let raw, !raw.isEmpty else { return raw }
    let separator = locale.decimalSeparator ?? "."
    // A canonical dot remains usable after a draft moves between locales.
    guard separator == "." || !raw.contains(separator) || !raw.contains(".") else { return raw }
    let text = raw.replacingOccurrences(of: separator, with: ".")
    let parts = text.split(separator: ".", omittingEmptySubsequences: false)
    guard parts.count <= 2, let first = parts.first, !first.isEmpty,
          parts.allSatisfy({ !$0.isEmpty }) else { return raw }
    var digits: [String] = []
    for part in parts {
      var translated = ""
      for character in part {
        guard character.unicodeScalars.count == 1,
              character.unicodeScalars.first?.properties.generalCategory == .decimalNumber,
              let value = character.wholeNumberValue else { return raw }
        translated += String(value)
      }
      digits.append(translated)
    }
    let integer = String(digits[0].drop(while: { $0 == "0" }))
    let whole = integer.isEmpty ? "0" : integer
    guard digits.count == 2 else { return whole }
    let fraction = String(digits[1].reversed().drop(while: { $0 == "0" }).reversed())
    return fraction.isEmpty ? whole : whole + "." + fraction
  }

  static func form(_ input: TeraAddForm, locale: Locale) -> TeraAddForm {
    guard input.commandType == .createFoodAvailability else { return input }
    var result = input
    result.priceAmount = canonicalOrRaw(input.priceAmount, locale: locale)
    result.quantity = canonicalOrRaw(input.quantity, locale: locale)
    return result
  }
}
