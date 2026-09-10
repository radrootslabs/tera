import Foundation

struct TeraTodayDiscoveryPresentation: Sendable, Equatable {
  private(set) var continuation: String?
  private(set) var hadIncompleteResponses = false
  private(set) var isSearching = false
  private(set) var hasSearched = false
  private(set) var failure: TeraTodayFailure?

  var canSearchOlder: Bool {
    continuation != nil && !isSearching
  }

  var message: String? {
    guard hasSearched else { return nil }
    if hadIncompleteResponses {
      return "Some relay responses were incomplete. Older searches may leave gaps."
    }
    if continuation != nil {
      return "More posts may be available. Search older posts to continue."
    }
    return "No more pages were returned in this search."
  }

  mutating func begin() {
    isSearching = true
    failure = nil
  }

  mutating func accept(_ receipt: TeraTodayDiscoveryReceipt) {
    continuation = receipt.continuation
    hadIncompleteResponses = receipt.hadIncompleteResponses
    hasSearched = true
    failure = nil
  }

  mutating func fail(_ error: Error) {
    let failure = TeraTodayFailure(error)
    self.failure = failure
    if failure.requiresRefresh {
      continuation = nil
    }
  }

  mutating func stop() {
    isSearching = false
  }
}
