import Foundation

enum TeraTodayCardPresentation {
  case feed
  case detail

  static let excerptCharacters = 640
  static let excerptBytes = 4096
  static let labelCharacters = 160
  static let labelBytes = 1024
  static let thumbnails = 3

  func content(_ value: String) -> String {
    self == .detail ? value : Self.excerpt(value, characters: Self.excerptCharacters, bytes: Self.excerptBytes)
  }

  func label(_ value: String) -> String {
    self == .detail ? value : Self.excerpt(value, characters: Self.labelCharacters, bytes: Self.labelBytes)
  }

  func media(_ values: [TeraMediaReference]) -> [TeraMediaReference] {
    self == .detail ? values : Array(values.prefix(Self.thumbnails))
  }

  func accessibility(_ card: TeraTodayCard) -> String {
    if self == .detail {
      return card.accessibilitySummary
    }
    var parts = [card.type.label, "by \(label(card.authorName))"]
    if let title = card.title {
      parts.append(label(title))
    }
    parts.append(content(card.content))
    if let price = card.priceSummary {
      parts.append(label(price))
    }
    if card.lifecycle != .active {
      parts.append(card.lifecycle.rawValue)
    }
    if let operation = card.localOperationState {
      parts.append(label(operation))
    }
    return parts.joined(separator: ", ")
  }

  /// Bound bytes before walking graphemes: a single combining sequence can be
  /// arbitrarily long. Drop an incomplete final cluster rather than split it.
  static func excerpt(_ value: String, characters: Int, bytes: Int) -> String {
    precondition(characters > 0 && bytes >= 3)
    let prefix = Array(value.utf8.prefix(bytes + 1))
    let byteTruncated = prefix.count > bytes
    var bounded: String
    if byteTruncated {
      var encoded = Array(prefix.prefix(bytes - 3))
      while String(bytes: encoded, encoding: .utf8) == nil {
        encoded.removeLast()
      }
      bounded = String(bytes: encoded, encoding: .utf8) ?? ""
      if !bounded.isEmpty {
        bounded.removeLast()
      }
    } else {
      bounded = value
    }
    let clusterPrefix = bounded.prefix(characters + 1)
    let truncated = byteTruncated || clusterPrefix.count > characters
    return truncated ? String(clusterPrefix.prefix(characters - 1)) + "…" : String(clusterPrefix)
  }
}
