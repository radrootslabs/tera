import XCTest

extension TeraRemoteQualificationUITests {
  @MainActor
  func beginNewDraft(_ action: XCUIElement) -> Bool {
    let ready = NSPredicate { _, _ in
      action.exists && action.isEnabled && action.isHittable
    }
    guard XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: ready, object: action)], timeout: 10) == .completed else { return false }
    action.tap()
    // New may first preserve the old editor through an asynchronous durable
    // save. Editing before it settles intentionally cancels that replacement.
    return XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: ready, object: action)], timeout: 60) == .completed
  }

  @MainActor
  func enterExactText(_ field: XCUIElement, value: String) {
    field.typeText(value)
    XCTAssertEqual(field.value as? String, value, "The fresh form must contain only the supplied field value")
  }

  @MainActor
  func openAdd(_ app: XCUIApplication) -> XCUIElement? {
    let add = app.tabBars.buttons["Add"]
    guard add.waitForExistence(timeout: 10) else { return nil }
    let root = app.descendants(matching: .any)["radroots.add.root"]
    let type = app.descendants(matching: .any)["radroots.add.type"]
    for _ in 0 ..< 3 {
      add.tap()
      guard root.waitForExistence(timeout: 10) else { continue }
      // Public disclosure precedes the picker in the lazy native Form.
      // Reveal its actual row before requiring existence and interaction.
      scrollTo(app, element: type)
      if type.waitForExistence(timeout: 10), type.isHittable {
        return type
      }
    }
    return nil
  }

  @MainActor
  func recordAccessibilityIssue(_ issue: XCUIAccessibilityAuditIssue) {
    let description = [
      "Audit type: \(issue.auditType.rawValue)",
      issue.compactDescription,
      issue.detailedDescription,
      issue.element?.debugDescription ?? "No associated accessibility element",
    ].joined(separator: "\n")
    let attachment = XCTAttachment(string: description)
    attachment.name = "Accessibility issue details"
    attachment.lifetime = .keepAlways
    add(attachment)
  }

  @MainActor
  func assertDraftOutboxContainsAtLeast(_ app: XCUIApplication, count: Int) {
    let sheet = app.descendants(matching: .any)["radroots.add.drafts.sheet"]
    let list = sheet.descendants(matching: .collectionView).firstMatch
    guard list.waitForExistence(timeout: 10) else {
      return XCTFail("The visible Drafts list was unavailable")
    }

    for _ in 0 ..< 8 {
      list.swipeDown()
    }
    var identifiers = Set<String>()
    let rows = app.descendants(matching: .any).matching(
      NSPredicate(format: "identifier BEGINSWITH 'tera.add.submission.' AND NOT identifier CONTAINS '.progress'")
    )
    for _ in 0 ..< 12 where identifiers.count < count {
      for index in 0 ..< rows.count {
        let identifier = rows.element(boundBy: index).identifier
        if identifier.range(of: "^tera\\.add\\.submission\\.[0-9a-f]{32}$", options: .regularExpression) != nil {
          identifiers.insert(identifier)
        }
      }
      if identifiers.count < count {
        list.swipeUp()
      }
    }
    XCTAssertGreaterThanOrEqual(
      identifiers.count,
      count,
      "The visible Drafts list omitted persisted outbox rows; identifiers=\(identifiers.sorted())"
    )
  }

  @MainActor
  func waitForWorkToFinish(
    _ app: XCUIApplication,
    submit: XCUIElement,
    status: XCUIElement,
    priorStatusLabel: String?,
    priorSubmitValue: String?
  ) -> Bool {
    let addRoot = app.descendants(matching: .any)["radroots.add.root"]
    let progress = app.descendants(matching: .any)["radroots.add.progress"]
    let submissionProgress = app.descendants(matching: .any)["tera.add.submission.progress"]
    let started = NSPredicate { _, _ in
      if progress.exists || submissionProgress.exists || addRoot.value as? String == "Working" {
        return true
      }
      if status.exists, !status.label.isEmpty, status.label != priorStatusLabel {
        return true
      }
      return submit.exists && submit.value as? String != priorSubmitValue
    }
    let startExpectation = XCTNSPredicateExpectation(predicate: started, object: app)
    _ = XCTWaiter.wait(for: [startExpectation], timeout: 5)
    let settled = NSPredicate { _, _ in
      addRoot.value as? String == "Ready" && !progress.exists && !submissionProgress.exists
    }
    let settleExpectation = XCTNSPredicateExpectation(predicate: settled, object: app)
    return XCTWaiter.wait(for: [settleExpectation], timeout: 180) == .completed
  }

  @MainActor
  func terminalControlShowsSettledState(_ submit: XCUIElement) -> Bool {
    if submit.isEnabled {
      return true
    }
    // Both complete and temporarily deferred work disable a new effect.
    // Successful qualification still requires actual policy completion below.
    let recognized = ["Delivery policy complete", "Publication waiting"].contains(submit.label)
    XCTAssertTrue(recognized, "Unexpected inactive publication control: \(submit.label)")
    return recognized
  }

  @MainActor
  func finishOriginalPublication(_ app: XCUIApplication, submit: XCUIElement) -> Bool {
    for _ in 0 ..< 3 {
      scrollTo(app, element: submit)
      guard submit.waitForExistence(timeout: 10) else { return false }
      if submit.label == "Delivery policy complete" {
        return true
      }
      guard submit.label == "Publication waiting" else {
        XCTFail("The original publication did not complete: \(submit.label)")
        return false
      }
      let priorValue = submit.value as? String
      let check = app.buttons["Check saved submission status"]
      TeraAccessibilityNavigation.scroll(app, to: check, up: false, test: self)
      guard check.waitForExistence(timeout: 10), check.isHittable else { return false }
      check.tap()
      guard waitForWorkToFinish(app, submit: submit, status: app.staticTexts["tera.add.submission.status"],
                                priorStatusLabel: nil, priorSubmitValue: priorValue) else { return false }
      scrollTo(app, element: submit)
      guard submit.waitForExistence(timeout: 10) else { return false }
      if submit.isEnabled {
        guard let ready = readySubmit(app), submitAndWait(app, submit: ready) != nil else { return false }
      }
    }
    scrollTo(app, element: submit)
    let completed = submit.exists && submit.label == "Delivery policy complete"
    XCTAssertTrue(completed, "The original publication did not complete within three visible status/continuation attempts")
    return completed
  }

  @MainActor
  func assertSavedPhotoEditing(_ app: XCUIApplication) {
    guard openDrafts(app) else { return XCTFail("The saved editing inventory did not open") }
    let reopen = app.buttons.matching(
      NSPredicate(format: "identifier BEGINSWITH 'tera.add.composer.' AND label == 'Reopen'")
    ).firstMatch
    guard reopen.waitForExistence(timeout: 20) else {
      return XCTFail("The persisted photo editing inventory omitted its Reopen action")
    }
    scrollTo(app, element: reopen)
    XCTAssertTrue(waitUntilHittable(reopen, timeout: 10))
    reopen.tap()
    XCTAssertTrue(app.navigationBars["Drafts & outbox"].waitForNonExistence(timeout: 20))
    let description = app.descendants(matching: .any)["Describe this photo"]
    scrollTo(app, element: description)
    XCTAssertEqual(description.value as? String, "A green test image prepared for the photo update",
                   "Reopening persisted photo editing must retain the supplied description")
  }
}
