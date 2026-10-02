import Foundation

struct TeraFoodPresentation {
  var locale: Locale = .current

  func price(_ card: TeraTodayCard) -> String? {
    guard let amount = card.priceAmount, let currency = card.priceCurrency, let unit = card.priceUnit else { return nil }
    guard let display = TeraExactDecimalPresentation(locale: locale).string(amount) else { return "Amount unavailable" }
    return "\(display) \(currency)/\(unit)"
  }

  func quantity(_ card: TeraTodayCard) -> String? {
    guard let quantity = card.quantity, let unit = card.priceUnit else { return nil }
    guard let display = TeraExactDecimalPresentation(locale: locale).string(quantity) else { return "Quantity unavailable" }
    return "\(display) \(unit) available"
  }
}
