import XCTest

extension TeraRemoteQualificationUITests {
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
      addRoot.value as? String == "Ready" && !progress.exists && !submissionProgress.exists && submit.isEnabled
    }
    let settleExpectation = XCTNSPredicateExpectation(predicate: settled, object: app)
    return XCTWaiter.wait(for: [settleExpectation], timeout: 180) == .completed
  }

  @MainActor
  func assertSavedPhotoEditing(_ app: XCUIApplication) {
    guard openDrafts(app) else { return XCTFail("The saved editing inventory did not open") }
    let savedPhoto = app.descendants(matching: .any).matching(
      NSPredicate(format: "identifier BEGINSWITH 'tera.add.composer.'")
    ).firstMatch
    XCTAssertTrue(savedPhoto.waitForExistence(timeout: 20))
    app.buttons["Done"].tap()
  }
}
