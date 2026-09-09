import Foundation

enum TeraTodayContentAvailability: Sendable, Equatable {
  case notLoaded
  case empty
  case available
}

enum TeraTodayRefreshState: Sendable, Equatable {
  case idle
  case refreshing
  case completed
  case failed(TeraTodayFailure)
}

enum TeraTodayFreshness: Sendable, Equatable {
  case unconfirmed
  /// A local projection receipt does not prove complete relay history.
  case refreshed(contentGeneration: UInt64)
}

enum TeraTodayFailure: Sendable, Equatable {
  case offline(message: String)
  case failed(message: String)
  case staleCursor(message: String)

  init(_ error: Error) {
    let message = TeraUserMessages.text(for: error, fallback: .todayUnavailable)
    switch TeraRuntimeFailure.from(error)?.recovery.disposition {
    case .staleCursor:
      self = .staleCursor(message: message)
    case .networkUnavailable:
      self = .offline(message: message)
    default:
      self = .failed(message: message)
    }
  }

  var message: String {
    switch self {
    case let .offline(message), let .failed(message), let .staleCursor(message): message
    }
  }

  var systemImage: String {
    switch self {
    case .offline: "wifi.slash"
    case .failed: "exclamationmark.triangle"
    case .staleCursor: "arrow.clockwise"
    }
  }

  var requiresRefresh: Bool {
    if case .staleCursor = self {
      return true
    }
    return false
  }

  var readStatus: String {
    requiresRefresh ? message : "Saved posts could not be read. \(message)"
  }
}

struct TeraTodayPresentation: Sendable, Equatable {
  private(set) var content: TeraTodayContentAvailability = .notLoaded
  private(set) var refresh: TeraTodayRefreshState = .idle
  private(set) var freshness: TeraTodayFreshness = .unconfirmed
  private(set) var readFailure: TeraTodayFailure?
  private(set) var isReading = false

  mutating func beginReload() {
    readFailure = nil
    isReading = false
    freshness = .unconfirmed
    if refresh == .refreshing {
      refresh = .idle
    }
  }

  mutating func beginRefresh() {
    refresh = .refreshing
  }

  mutating func refreshCompleted() {
    refresh = .completed
  }

  mutating func refreshFailed(_ error: Error) {
    refresh = .failed(TeraTodayFailure(error))
  }

  mutating func beginRead() {
    isReading = true
  }

  mutating func finishReading() {
    isReading = false
  }

  mutating func acceptPage(count: Int, receipt: TeraTodayRefreshReceipt? = nil) {
    content = count == 0 ? .empty : .available
    readFailure = nil
    isReading = false
    if let receipt {
      freshness = .refreshed(contentGeneration: receipt.contentGeneration)
    }
  }

  mutating func failRead(_ failure: TeraTodayFailure) {
    readFailure = failure
    isReading = false
    freshness = .unconfirmed
  }

  mutating func stop() {
    isReading = false
    if refresh == .refreshing {
      refresh = .idle
    }
  }

  var freshnessMessage: String? {
    guard content != .notLoaded else { return nil }
    switch freshness {
    case .unconfirmed: return "Showing saved posts."
    case .refreshed: return "Saved posts checked."
    }
  }

  var accessibilityStatus: String {
    var messages: [String] = []
    if refresh == .refreshing {
      messages.append("Checking for updates.")
    }
    if isReading {
      messages.append("Reading saved posts.")
    }
    if case let .failed(failure) = refresh {
      messages.append("Refresh failed. \(failure.message)")
    }
    if let readFailure {
      messages.append(readFailure.readStatus)
    }
    if let freshnessMessage {
      messages.append(freshnessMessage)
    }
    return messages.joined(separator: " ")
  }
}
