import XCTest

final class TeraRecoveryUITests: XCTestCase {
  @MainActor
  func testUnknownLockedQuotaAndMismatchKeepEditingAndExposeSelectedCheck() {
    let app = XCUIApplication()
    app.launchEnvironment["TERA_IOS_UI_TEST_SHELL"] = "1"
    app.launchEnvironment["TERA_IOS_UI_TEST_RECOVERY"] = "1"
    app.launch()
    XCTAssertTrue(app.tabBars.firstMatch.waitForExistence(timeout: 5))
    app.tabBars.firstMatch.buttons["Add"].tap()
    let key = String(repeating: "a", count: 64)
    let editing = app.textFields["tera.test.recovery.editing"]
    XCTAssertTrue(editing.waitForExistence(timeout: 5))
    let reason = app.staticTexts["tera.add.recovery.reason.\(key)"]
    XCTAssertTrue(reason.label.contains("not yet confirmed"))
    XCTAssertTrue(app.staticTexts["tera.add.recovery.remaining"].label.contains("12 more saved transfer records"))
    let check = app.buttons["tera.add.recovery.check.\(key)"]
    XCTAssertTrue(check.isEnabled)
    check.tap()
    XCTAssertEqual(app.staticTexts["tera.test.recovery.checked"].label, key)
    let next = app.buttons["tera.test.recovery.next"]
    next.tap()
    XCTAssertTrue(app.staticTexts["tera.add.recovery.message"].label.contains("device is unlocked"))
    next.tap()
    XCTAssertTrue(app.staticTexts["tera.add.recovery.message"].label.contains("storage is full"))
    next.tap()
    XCTAssertTrue(reason.label.contains("does not match"))
    XCTAssertEqual(editing.value as? String, "Keep editing during recovery")
    XCTAssertTrue(check.isEnabled)
  }
}
