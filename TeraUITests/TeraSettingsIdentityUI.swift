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
      settings.swipeUp()
    }
    return false
  }
}
