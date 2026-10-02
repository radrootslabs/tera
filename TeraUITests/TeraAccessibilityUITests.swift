import XCTest

@MainActor
final class TeraAccessibilityUITests: XCTestCase {
  private var auditFindings: [String] = []

  override func tearDown() async throws {
    continueAfterFailure = true
    XCTAssertTrue(auditFindings.isEmpty, "Unresolved accessibility findings:\n" + auditFindings.joined(separator: "\n"))
    auditFindings.removeAll()
    try await super.tearDown()
  }

  func testFormUpdateAtLargestTextWithReduceMotionAndLocalSave() throws {
    try qualifyForm("Update")
  }

  func testFormPhotoUpdateAtLargestTextWithReduceMotionAndLocalSave() throws {
    try qualifyForm("Photo update")
  }

  func testFormAskAtLargestTextWithReduceMotionAndLocalSave() throws {
    try qualifyForm("Ask")
  }

  func testFormEventAtLargestTextWithReduceMotionAndLocalSave() throws {
    try qualifyForm("Event")
  }

  func testFormFoodAvailabilityAtLargestTextWithReduceMotionAndLocalSave() throws {
    try qualifyForm("Food availability")
  }

  func testUnconfirmedSaveStatusAtLargestText() throws {
    let app = launch(scenario: "save-error", form: "Photo update")
    XCTAssertTrue(app.tabBars.buttons["Add"].waitForExistence(timeout: 15))
    app.tabBars.buttons["Add"].tap()
    let prepare = app.buttons["Prepare unconfirmed Save"]
    XCTAssertTrue(prepare.waitForExistence(timeout: 5))
    prepare.tap()
    XCTAssertTrue(prepare.waitForNonExistence(timeout: 5))
    let error = app.staticTexts["The Add operation could not be completed."]
    XCTAssertEqual(error.label, "The Add operation could not be completed.")
    let state = app.descendants(matching: .any)["tera.add.save-state"]
    XCTAssertTrue(state.waitForExistence(timeout: 5))
    XCTAssertEqual(state.label, "Changes are not confirmed saved. Use Save draft to retry.")
    scroll(app, to: state)
    try audit(app)
  }

  func testActualPhotoDescriptionRemainsEditableAcrossSaveFailureAndRecovery() throws {
    let app = launch(form: "Photo update")
    XCTAssertTrue(app.tabBars.buttons["Add"].waitForExistence(timeout: 15))
    app.tabBars.buttons["Add"].tap()
    let library = app.buttons["Photo Library"]
    scroll(app, to: library)
    XCTAssertTrue(library.isHittable)
    library.tap()
    let description = app.descendants(matching: .any)["Describe this photo"]
    scroll(app, to: description, up: false)
    XCTAssertTrue(description.isHittable)
    description.tap()
    description.typeText("Carrots arranged in a wooden crate")
    app.buttons["Done"].tap()
    XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 5))
    XCTAssertEqual(description.value as? String, "Carrots arranged in a wooden crate")
    try audit(app)
    let save = app.buttons["Save draft"]
    scroll(app, to: save)
    save.tap()
    let failed = app.descendants(matching: .any)["tera.add.save-state"]
    XCTAssertTrue(failed.waitForExistence(timeout: 5))
    XCTAssertTrue(failed.isHittable, "Explicit Save must reveal its acknowledgment")
    scroll(app, to: failed, up: false)
    XCTAssertEqual(failed.label, "Changes are not confirmed saved. Use Save draft to retry.")
    try audit(app)
    scroll(app, to: description)
    XCTAssertEqual(description.value as? String, "Carrots arranged in a wooden crate")
    let remove = app.buttons["Remove photo"]
    scroll(app, to: remove)
    remove.tap()
    scroll(app, to: save)
    save.tap()
    let saved = app.descendants(matching: .any)["tera.add.save-state"]
    XCTAssertTrue(saved.waitForExistence(timeout: 5))
    XCTAssertTrue(saved.isHittable, "Explicit Save must reveal its acknowledgment")
    scroll(app, to: saved, up: false)
    XCTAssertEqual(saved.label, "Changes saved on this device.")
  }

  func testRuntimeFailureActionIsReachableAtLargestText() throws {
    let app = launch(scenario: "failure")
    let retry = app.buttons["Retry"]
    XCTAssertTrue(retry.waitForExistence(timeout: 5))
    try traverse(app, until: retry, auditing: true)
    XCTAssertGreaterThanOrEqual(retry.frame.height, 44)
    retry.tap()
    XCTAssertTrue(app.staticTexts["Local state checked"].exists)
  }

  func testSupportSettingsAndDiagnosticsActionRemainReachableAtLargestText() throws {
    let app = launch()
    XCTAssertTrue(app.tabBars.buttons["Today"].waitForExistence(timeout: 15))
    app.tabBars.buttons["Today"].tap()
    app.buttons["Account"].tap()
    let settings = app.buttons["Settings"]
    scroll(app, to: settings)
    settings.tap()
    let diagnostics = app.buttons["Prepare diagnostics export"]
    scroll(app, to: diagnostics)
    XCTAssertGreaterThanOrEqual(diagnostics.frame.height, 44)
    try audit(app)
    auditFindings += try TeraAccessibilityNavigation.qualifySupportSettings(app, test: self)
    let back = app.navigationBars.buttons["Me"]
    XCTAssertTrue(back.waitForExistence(timeout: 5))
    back.tap()
    XCTAssertTrue(app.buttons["Done"].waitForExistence(timeout: 5))
    app.buttons["Done"].tap()
    XCTAssertTrue(app.tabBars.buttons["Today"].waitForExistence(timeout: 5))
  }

  func testSecureIdentityImportScalesAndCanBeDismissedWithoutSubmitting() throws {
    let app = launch(scenario: "identity")
    let open = app.buttons["Import identity"]
    XCTAssertTrue(open.waitForExistence(timeout: 5))
    try traverse(app, until: open, auditing: true)
    open.tap()
    for instruction in ["Enter an nsec or 64-character secret key.",
                        "It is transferred directly to Apple custody.",
                        "Secret input is never stored in view state."]
    {
      let text = app.staticTexts.matching(NSPredicate(format: "label == %@", instruction)).firstMatch
      scroll(app, to: text)
      try audit(app)
    }
    let field = app.secureTextFields["Secret identity key"]
    scroll(app, to: field)
    XCTAssertTrue(field.exists)
    XCTAssertTrue(field.isHittable)
    XCTAssertGreaterThanOrEqual(field.frame.height, 44)
    try audit(app)
    let submit = app.buttons["Import securely"]
    scroll(app, to: submit)
    XCTAssertTrue(submit.isHittable)
    XCTAssertGreaterThanOrEqual(submit.frame.height, 44)
    scroll(app, to: field, up: false)
    field.tap()
    field.typeText("invalid-key\n")
    XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
    XCTAssertTrue(app.staticTexts["Enter a valid Nostr secret key."].waitForExistence(timeout: 5))
    app.buttons["Cancel"].tap()
    XCTAssertTrue(open.waitForExistence(timeout: 5))
  }
}

private extension TeraAccessibilityUITests {
  func qualifyForm(_ type: String) throws {
    let app = launch(form: type)
    XCTAssertTrue(app.tabBars.buttons["Add"].waitForExistence(timeout: 15), app.debugDescription)
    if type == "Update" {
      app.tabBars.buttons["Today"].tap()
      try audit(app, dynamicType: true)
    }
    app.tabBars.buttons["Add"].tap()
    verifyEditingFocus(app, type: type)
    try audit(app)
    let save = app.buttons["Save draft"]
    let disclosure = app.staticTexts["Submit saves an immutable local snapshot first."]
    scroll(app, to: disclosure)
    try audit(app)
    // Audits may move a lazy form back to its earlier rows. Seek the action
    // forward first; the bounded navigator reverses at an observed edge.
    scroll(app, to: save)
    XCTAssertTrue(save.isEnabled)
    XCTAssertGreaterThanOrEqual(save.frame.height, 44)
    save.tap()
    let saved = app.descendants(matching: .any)["tera.add.save-state"]
    XCTAssertTrue(saved.waitForExistence(timeout: 5))
    XCTAssertTrue(saved.isHittable, "Explicit Save must reveal its acknowledgment")
    scroll(app, to: saved, up: false)
    XCTAssertEqual(saved.label, "Changes saved on this device.")
    XCTAssertTrue(saved.isHittable)
    try audit(app)
    app.terminate()
  }

  func launch(scenario: String = "forms", form: String = "Update") -> XCUIApplication {
    continueAfterFailure = false
    _ = TeraAccessibilitySettings.setSystemReduceMotion(true, restoring: self)
    let app = XCUIApplication()
    app.launchEnvironment["TERA_IOS_UI_TEST_SHELL"] = "1"
    app.launchEnvironment["TERA_IOS_UI_TEST_ACCESSIBILITY"] = scenario
    app.launchEnvironment["TERA_IOS_UI_TEST_FORM"] = form
    app.launchArguments = ["-AppleLanguages", "(en)", "-AppleLocale", "en_US",
                           "-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL"]
    app.launch()
    let root = app.otherElements["tera.test.accessibility.root"]
    XCTAssertTrue(root.waitForExistence(timeout: 5))
    let environment = XCTAttachment(string: "Observed presentation environment: \(root.value ?? "missing")")
    environment.lifetime = .keepAlways
    add(environment)
    XCTAssertEqual(root.value as? String, "text=accessibility5; reduceMotion=true")
    return app
  }

  func verifyEditingFocus(_ app: XCUIApplication, type: String) {
    let labels = ["Update": "What’s happening locally?", "Photo update": "What should neighbors know?",
                  "Ask": "What do you need or want to know?", "Event": "Event details (optional)",
                  "Food availability": "Details"]
    let content = app.textViews[labels[type]!]
    scroll(app, to: content)
    XCTAssertEqual(content.label, labels[type])
    XCTAssertGreaterThanOrEqual(content.frame.height, 44)
    content.tap()
    XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
    content.typeText("Seedlings available nearby.")
    XCTAssertEqual(content.value as? String, "Seedlings available nearby.")
    app.buttons["Done"].tap()
    XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 5))
    let traversal = XCTAttachment(string: app.debugDescription)
    traversal.name = "\(type) editing accessibility tree"
    traversal.lifetime = .keepAlways
    add(traversal)
  }

  func audit(_ app: XCUIApplication, dynamicType: Bool = false) throws {
    auditFindings += try TeraAccessibilityAudit(test: self).run(app, dynamicType: dynamicType)
  }

  func traverse(_ app: XCUIApplication, until element: XCUIElement, auditing: Bool) throws {
    scroll(app, to: element)
    if auditing {
      try audit(app)
      // Audits can move accessibility focus and scroll position. Restore the
      // requested action before interacting with it.
      scroll(app, to: element)
    }
  }

  func scroll(_ app: XCUIApplication, to element: XCUIElement, up: Bool = true) {
    TeraAccessibilityNavigation.scroll(app, to: element, up: up, test: self)
  }
}
