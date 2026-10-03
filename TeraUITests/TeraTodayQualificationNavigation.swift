import XCTest

@MainActor
enum TeraTodayQualificationNavigation {
  static func unseenMarkers(
    _ app: XCUIApplication,
    feed: XCUIElement,
    markers: [String],
    maximumCards: Int
  ) -> Set<String> {
    var unseen = Set(markers)
    guard (1 ... 15).contains(maximumCards) else {
      XCTFail("The governed Today navigation card bound was invalid")
      return unseen
    }
    // The closed fixture admits at most15 cards; AX5 rows span several screens.
    // Observe every marker while traversing, without restarting for each one.
    for forward in [true, false] {
      for _ in 0 ..< maximumCards * 6 {
        unseen = unseen.filter { marker in
          !app.descendants(matching: .any).matching(
            NSPredicate(format: "label CONTAINS %@", marker)
          ).firstMatch.exists
        }
        if unseen.isEmpty {
          return unseen
        }
        if forward {
          feed.swipeUp()
        } else {
          feed.swipeDown()
        }
      }
    }
    return unseen
  }
}
