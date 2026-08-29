import XCTest

final class RadrootsRootShellUITests: XCTestCase {
    @MainActor
    func testShellExposesExactlyTodayAndAddBottomTabs() {
        let app = XCUIApplication()
        app.launchEnvironment["RADROOTS_IOS_UI_TEST_SHELL"] = "1"
        app.launch()

        let tabBar = app.tabBars.firstMatch
        XCTAssertTrue(tabBar.waitForExistence(timeout: 5))
        XCTAssertEqual(tabBar.buttons.count, 2)
        XCTAssertTrue(tabBar.buttons["Today"].exists)
        XCTAssertTrue(tabBar.buttons["Add"].exists)
        XCTAssertFalse(tabBar.buttons["Capture"].exists)
        XCTAssertFalse(tabBar.buttons["Activity"].exists)
        XCTAssertFalse(tabBar.buttons["Settings"].exists)

        tabBar.buttons["Add"].tap()
        XCTAssertTrue(
            app.descendants(matching: .any)["radroots.add.root"].waitForExistence(timeout: 2)
        )
        tabBar.buttons["Today"].tap()
        XCTAssertTrue(
            app.descendants(matching: .any)["radroots.today.root"].waitForExistence(timeout: 2)
        )
    }

    @MainActor
    func testCanonicalDeepLinksDriveLifecycleNavigation() throws {
        let app = XCUIApplication()
        app.launchEnvironment["RADROOTS_IOS_UI_TEST_SHELL"] = "1"
        app.launch()

        let tabBar = app.tabBars.firstMatch
        let today = app.descendants(matching: .any)["radroots.today.root"]
        let add = app.descendants(matching: .any)["radroots.add.root"]
        XCTAssertTrue(tabBar.waitForExistence(timeout: 5))
        tabBar.buttons["Today"].tap()
        XCTAssertTrue(today.waitForExistence(timeout: 5))

        try app.open(XCTUnwrap(URL(string: "radroots://add")))
        XCTAssertTrue(add.waitForExistence(timeout: 2))

        try app.open(XCTUnwrap(URL(string: "radroots://today")))
        XCTAssertTrue(today.waitForExistence(timeout: 2))

        try app.open(XCTUnwrap(URL(string: "radroots://add/extra")))
        XCTAssertTrue(today.waitForExistence(timeout: 2))
        XCTAssertFalse(add.exists)
    }
}
