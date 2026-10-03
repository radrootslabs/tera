import XCTest

extension TeraRemoteQualificationUITests {
  @MainActor
  func recoverRejectedEvent(_ app: XCUIApplication, content: XCUIElement, marker: String, failure: String) throws {
    XCTAssertTrue(failure.contains("Error code"))
    scrollTo(app, element: content)
    XCTAssertEqual(content.value as? String, marker)
    XCTAssertTrue(failure.contains("event_title_required"))
    XCTAssertTrue(failure.contains("Original request retained"))
    let identifier = content.identifier
    // Corrected editing is a new explicit request, never a mutation of the
    // already reserved command and composer revision rejected above.
    try beginDraft(app, type: "Event")
    try enterText(app, identifier: identifier, value: marker)
  }
}
