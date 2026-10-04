import XCTest

extension TeraRemoteQualificationUITests {
  @MainActor
  func openSettingsFromMe(
    _ app: XCUIApplication,
    meSheet: XCUIElement
  ) -> Bool {
    let meList = meSheet.descendants(matching: .collectionView).firstMatch
    guard meList.waitForExistence(timeout: 10) else { return false }
    let settings = app.descendants(matching: .any)["radroots.support.settings"]
    for _ in 0 ..< 8 {
      if settings.exists,
        settings.isHittable,
        settings.frame.minY >= meSheet.frame.minY,
        settings.frame.maxY <= meSheet.frame.maxY
      {
        settings.tap()
        return app.descendants(matching: .any)["radroots.support.settings.view"]
          .waitForExistence(timeout: 20)
      }
      meList.swipeUp()
    }
    return false
  }

  @MainActor
  func exerciseSettingsIdentityOverscroll(
    _ app: XCUIApplication,
    expectedPublicKey: String
  ) throws {
    app.tabBars.buttons["Today"].tap()
    let account = app.descendants(matching: .any)["radroots.support.account"]
    XCTAssertTrue(account.waitForExistence(timeout: 10))
    account.tap()
    let meSheet = app.descendants(matching: .any)["radroots.support.me.sheet"]
    XCTAssertTrue(meSheet.waitForExistence(timeout: 10))
    XCTAssertTrue(openSettingsFromMe(app, meSheet: meSheet))

    let settings = app.descendants(matching: .any)["radroots.support.settings.view"]
    let profile = app.descendants(matching: .any)["radroots.settings.profile.name"]
    let navigation = app.navigationBars["Settings"]
    for _ in 0 ..< 12 {
      if profile.exists, profile.isHittable,
        profile.frame.minY >= max(settings.frame.minY, navigation.frame.maxY),
        profile.frame.maxY <= settings.frame.maxY
      {
        break
      }
      settings.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.75))
        .press(
          forDuration: 0.1,
          thenDragTo: settings.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.35))
        )
    }
    _ = try XCTUnwrap(
      profile.exists && profile.isHittable ? profile : nil,
      "The native overscroll regression must reach the later Profile section"
    )
    let publicKey = app.descendants(matching: .any)["radroots.settings.identity.public_key"]
    let revealed = revealSettingsPublicKey(app, publicKey: publicKey)
    XCTAssertTrue(revealed, "Settings identity navigation must recover after overscroll")
    guard revealed else { return }
    XCTAssertEqual(publicKey.value as? String, expectedPublicKey)

    let back = app.navigationBars.buttons["Me"].firstMatch
    XCTAssertTrue(back.waitForExistence(timeout: 10))
    back.tap()
    let done = app.navigationBars.buttons["Done"].firstMatch
    XCTAssertTrue(done.waitForExistence(timeout: 10))
    done.tap()
    XCTAssertTrue(meSheet.waitForNonExistence(timeout: 10))
  }

  @MainActor
  func revealSettingsPublicKey(_ app: XCUIApplication, publicKey: XCUIElement) -> Bool {
    let settings = app.descendants(matching: .any)["radroots.support.settings.view"]
    guard settings.waitForExistence(timeout: 10) else { return false }
    let navigation = app.navigationBars["Settings"]
    // Settings uses lazy rows. Read the real identity only after revealing it
    // through the same visible scroll surface available to the user at AX5.
    for _ in 0 ..< 8 {
      if publicKey.exists, publicKey.isHittable,
        publicKey.frame.minY >= max(settings.frame.minY, navigation.frame.maxY),
        publicKey.frame.maxY <= settings.frame.maxY
      {
        return true
      }
      let aboveViewport = publicKey.exists && publicKey.frame.height > 0
        && publicKey.frame.minY < max(settings.frame.minY, navigation.frame.maxY)
      let laterSectionVisible = [
        "radroots.settings.profile.name",
        "radroots.settings.network.environment",
        "radroots.settings.relays.add",
        "radroots.settings.retry.network",
        "radroots.settings.blossom.primary",
      ].contains { identifier in
        let row = app.descendants(matching: .any)[identifier]
        return row.exists && row.isHittable
      }
      let movingBack = aboveViewport || laterSectionVisible
      // Overlapping drags retain the same eight checks without skipping a
      // large lazy row, and return when the real row has passed the viewport.
      settings.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: movingBack ? 0.35 : 0.75))
        .press(
          forDuration: 0.1,
          thenDragTo: settings.coordinate(
            withNormalizedOffset: CGVector(dx: 0.5, dy: movingBack ? 0.75 : 0.35)
          )
        )
    }
    return false
  }

  @MainActor
  func reachRoot(_ app: XCUIApplication) {
    for _ in 0 ..< 6 {
      if app.tabBars.firstMatch.waitForExistence(timeout: 3) {
        return
      }
      for identifier in [
        "radroots.identity.create",
        "radroots.identity.unlock",
        "radroots.configuration.reconfigure",
        "radroots.identity.recover",
        "radroots.runtime.retry",
      ] {
        let action = app.descendants(matching: .any)[identifier]
        if action.exists, action.isHittable {
          action.tap()
          break
        }
      }
    }
    XCTFail("Tera did not reach the two-tab root without interactive authentication")
  }
}
