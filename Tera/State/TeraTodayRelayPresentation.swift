import Foundation

extension TeraTodayPresentation {
  var relayMessages: [String] {
    guard let receipt = relayReceipt else { return [] }
    let message: String
    switch receipt.relayState {
    case .complete: return []
    case .partial: message = "Refresh incomplete. Showing available posts."
    case .offline: message = "Network refresh unavailable. Showing saved posts."
    }
    return [message] + receipt.targets.enumerated().map { index, target in
      "Relay \(index + 1): \(target.statusMessage)"
    }
  }
}

extension TeraTodayTargetSyncReceipt {
  var statusMessage: String {
    guard let summary, summary.pagesObserved > 0 else { return "Response unconfirmed." }
    if summary.missingOutcomePages > 0 {
      return "Some responses unconfirmed."
    }
    if let state = summary.lastIncomplete {
      switch state {
      case .complete: return "Response unconfirmed."
      case .partial: return "Some requested posts were unavailable."
      case .unavailable: return "A response was unavailable."
      case .failedRetryable: return "A response failed. Try again."
      case .failedTerminal: return "A response failed."
      case .cancelled: return "A request was cancelled."
      }
    }
    guard summary.incompletePages == 0, finalState == .complete else { return "Response unconfirmed." }
    return "Returned pages finished."
  }
}
