import XCTest

extension TeraRemoteQualificationUITests {
  @MainActor
  func preparePhotoUpdate(_ app: XCUIApplication, marker: String) -> XCUIElement? {
    guard let type = openAdd(app) else {
      XCTFail("The Add bottom tab did not present the Add surface")
      return nil
    }
    let newDraft = app.buttons["radroots.add.new"]
    XCTAssertTrue(newDraft.waitForExistence(timeout: 10))
    newDraft.tap()

    guard selectPhotoUpdate(app, type: type) else { return nil }
    let content = app.descendants(matching: .any)["radroots.add.content"]
    XCTAssertTrue(content.waitForExistence(timeout: 10))
    guard waitUntilHittable(content, timeout: 10) else {
      XCTFail("Photo update content did not become hittable")
      return nil
    }
    content.tap()
    content.typeText(marker)
    let keyboardDone = app.buttons["radroots.add.keyboard.done"]
    XCTAssertTrue(keyboardDone.waitForExistence(timeout: 10))
    keyboardDone.tap()
    XCTAssertFalse(app.keyboards.firstMatch.waitForExistence(timeout: 5))

    let library = app.descendants(matching: .any)["radroots.add.media.library"]
    scrollTo(app, element: library)
    guard library.waitForExistence(timeout: 10), waitUntilHittable(library, timeout: 10) else {
      XCTFail("The Photo Library action did not become available through the visible UI")
      return nil
    }
    library.tap()
    let preparedStatus = app.staticTexts.matching(
      NSPredicate(format: "label == 'Photo prepared. Add descriptive text before publishing.'")
    ).firstMatch
    guard preparedStatus.waitForExistence(timeout: 60) else {
      XCTFail("The selected Photo update did not report the visible prepared state")
      return nil
    }
    let prepared = app.descendants(matching: .any)["radroots.add.media.prepared"]
    scrollTo(app, element: prepared)
    guard prepared.waitForExistence(timeout: 10) else {
      XCTFail("The selected Photo update did not reach the visible prepared state")
      return nil
    }
    do {
      try enterPhotoDescription(app)
    } catch {
      return nil
    }
    return readySubmit(app)
  }

  @MainActor
  private func selectPhotoUpdate(_ app: XCUIApplication, type: XCUIElement) -> Bool {
    for _ in 0 ..< 3 where !app.buttons["Photo update"].exists {
      type.tap()
      _ = app.buttons["Photo update"].waitForExistence(timeout: 10)
    }
    let photoUpdate = app.buttons["Photo update"]
    guard photoUpdate.exists else {
      XCTFail("The Add type picker did not present Photo update")
      return false
    }
    guard waitUntilHittable(photoUpdate, timeout: 10) else {
      XCTFail("Photo update did not become hittable")
      return false
    }

    var selectedPhotoUpdate = false
    for _ in 0 ..< 3 where !selectedPhotoUpdate {
      photoUpdate.tap()
      selectedPhotoUpdate = waitForValue(type, value: "Photo update", timeout: 10)
      if !selectedPhotoUpdate, !photoUpdate.exists {
        type.tap()
        _ = photoUpdate.waitForExistence(timeout: 10)
      }
    }
    guard selectedPhotoUpdate else {
      XCTFail("Photo update selection did not update the Add composer type")
      return false
    }
    return true
  }

  @MainActor
  func enterPhotoDescription(_ app: XCUIApplication) throws {
    try enterText(app, identifier: "radroots.add.media.alt", value: "A green test image prepared for the photo update")
  }
}
