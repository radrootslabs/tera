import XCTest

final class TeraEditingProtectionUITests: XCTestCase {
  @MainActor
  func testCancelDiscardConfirmationKeepsEditingAndExplicitDiscardAppliesOnce() {
    let app = XCUIApplication()
    app.launchEnvironment["TERA_IOS_UI_TEST_SHELL"] = "1"
    app.launchEnvironment["TERA_IOS_UI_TEST_EDITING_PROTECTION"] = "1"
    app.launch()
    let tabBar = app.tabBars.firstMatch
    XCTAssertTrue(tabBar.waitForExistence(timeout: 5))
    tabBar.buttons["Add"].tap()
    let editing = app.textFields["tera.test.editing.value"]
    XCTAssertTrue(editing.waitForExistence(timeout: 5))
    app.buttons["tera.test.editing.replace"].tap()
    let discard = app.buttons["tera.add.protection.discard"]
    XCTAssertTrue(discard.waitForExistence(timeout: 5))
    discard.tap()
    let confirmation = app.sheets.firstMatch
    XCTAssertTrue(confirmation.waitForExistence(timeout: 5))
    let keep = confirmation.buttons["Keep editing"]
    if keep.exists {
      keep.tap()
    } else {
      app.otherElements["PopoverDismissRegion"].coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.75)).tap()
    }
    XCTAssertTrue(confirmation.waitForNonExistence(timeout: 5))
    XCTAssertEqual(editing.value as? String, "Keep this unfinished text")
    XCTAssertEqual(app.staticTexts["tera.test.editing.replacements"].label, "0")
    XCTAssertTrue(discard.waitForNonExistence(timeout: 5))
    app.buttons["tera.test.editing.replace"].tap()
    XCTAssertTrue(discard.waitForExistence(timeout: 5))
    discard.tap()
    XCTAssertTrue(confirmation.waitForExistence(timeout: 5))
    confirmation.buttons["Discard and continue"].tap()
    let count = app.staticTexts["tera.test.editing.replacements"]
    let applied = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == '1'"), object: count)
    XCTAssertEqual(XCTWaiter.wait(for: [applied], timeout: 5), .completed)
    XCTAssertEqual(editing.value as? String, "Editing")
    XCTAssertTrue(discard.waitForNonExistence(timeout: 5))
  }
}
