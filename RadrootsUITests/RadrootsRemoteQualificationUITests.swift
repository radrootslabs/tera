import CryptoKit
import XCTest

final class RadrootsRemoteQualificationUITests: XCTestCase {
  private static let receiptName = "radroots-remote-qualification-bootstrap.json"

  @MainActor
  func testBootstrapIdentityWithoutInteractiveAuthentication() throws {
    let configuration = try QualificationConfiguration.environment()
    let app = launchToRoot(configuration)
    let publicKey = try readPublicKey(app)
    try writeBootstrapReceipt(configuration: configuration, publicKey: publicKey)

    XCUIDevice.shared.press(.home)
    app.activate()
    XCTAssertTrue(app.tabBars.firstMatch.waitForExistence(timeout: 10))

    app.terminate()
    app.launch()
    reachRoot(app)
    XCTAssertTrue(app.tabBars.firstMatch.waitForExistence(timeout: 20))
  }

  @MainActor
  func testStrictPublicEndpointRejectsPrivateAddress() throws {
    let environment = try QualificationConfiguration.environment()
    let configuration = QualificationConfiguration(
      runID: environment.runID,
      relayURLs: [],
      blossomOrigins: ["https://127.0.0.1"]
    )
    let app = XCUIApplication()
    app.launchEnvironment = configuration.launchEnvironment
    app.launch()

    let failureCode = app.descendants(matching: .any)["radroots.runtime.failure_code"]
    for _ in 0 ..< 8 where !failureCode.exists {
      for identifier in [
        "radroots.identity.create",
        "radroots.identity.unlock",
        "radroots.configuration.reconfigure",
        "radroots.identity.recover",
      ] {
        let action = app.descendants(matching: .any)[identifier]
        if action.exists, action.isHittable {
          action.tap()
          break
        }
      }
      _ = failureCode.waitForExistence(timeout: 4)
    }
    XCTAssertEqual(
      failureCode.label,
      "Error code runtime_failed",
      "A private Blossom endpoint did not expose the typed fail-closed result; "
        + "failure_code.label=\(failureCode.exists ? failureCode.label : "missing")"
    )
    XCTAssertFalse(app.tabBars.firstMatch.exists)
  }

  @MainActor
  func testLocalSocialFiveFlowScenario() throws {
    let configuration = try QualificationConfiguration.environment()
    let control = try XCTUnwrap(configuration.fixtureControl)
    try? FileManager.default.removeItem(atPath: control)
    var app = launchToRoot(configuration)
    let marker = "\(configuration.runID)-photo"

    guard preparePhotoUpdate(app, marker: marker) != nil else { return }
    let save = app.buttons["radroots.add.save"]
    scrollTo(app, element: save)
    guard save.waitForExistence(timeout: 10),
      waitUntilHittable(save, timeout: 10),
      isUnobscured(save, above: app.tabBars.firstMatch)
    else {
      return XCTFail("The prepared Photo update could not be saved through the visible UI")
    }
    let addRoot = app.descendants(matching: .any)["radroots.add.root"]
    save.tap()
    let saved = app.staticTexts.matching(identifier: "radroots.add.status").firstMatch
    let saveStarted = NSPredicate { _, _ in
      addRoot.value as? String == "Working"
        || saved.exists && saved.label == "Draft saved on this device."
    }
    let startExpectation = XCTNSPredicateExpectation(predicate: saveStarted, object: app)
    guard XCTWaiter.wait(for: [startExpectation], timeout: 10) == .completed else {
      return XCTFail("The visible Save draft action did not start durable persistence")
    }
    let saveSettled = NSPredicate { _, _ in
      addRoot.value as? String == "Ready"
    }
    let settleExpectation = XCTNSPredicateExpectation(predicate: saveSettled, object: app)
    guard XCTWaiter.wait(for: [settleExpectation], timeout: 60) == .completed else {
      return XCTFail("The visible Save draft action did not finish durable persistence")
    }
    scrollAddFormToTop(app)
    guard waitForLabel(saved, label: "Draft saved on this device.", timeout: 10) else {
      return XCTFail("The visible Add status did not confirm the durable draft save")
    }
    XCTAssertTrue(assertUnverifiedDraft(app))
    app.terminate()

    XCTAssertTrue(FileManager.default.createFile(atPath: control, contents: Data()))
    app = launchToRoot(configuration)
    guard openAdd(app) != nil, openDrafts(app) else {
      return XCTFail("The persisted media draft was unavailable after relaunch")
    }
    let reopen = app.buttons["Reopen"].firstMatch
    XCTAssertTrue(reopen.waitForExistence(timeout: 20))
    reopen.tap()
    guard let retrySubmit = readySubmit(app),
      let retryValue = submitAndWait(app, submit: retrySubmit)
    else { return }
    XCTAssertFalse(
      retryValue.contains("Error code"),
      "The preserved media draft did not publish after retry; submit.value=\(retryValue)"
    )

    let markers = [
      "\(configuration.runID)-update",
      "\(configuration.runID)-ask",
      "\(configuration.runID)-event",
      "\(configuration.runID)-food",
    ]
    try publishTextFlow(app, type: "Update", marker: markers[0])
    try publishTextFlow(app, type: "Ask", marker: markers[1])
    try publishEvent(app, marker: markers[2])
    try publishFood(app, marker: markers[3])

    let expected = [marker] + markers
    try assertTodayContains(app, markers: expected)
    app.terminate()
    app = launchToRoot(configuration)
    try assertTodayContains(app, markers: expected)

    guard openAdd(app) != nil, openDrafts(app) else {
      return XCTFail("The durable outbox was unavailable after the final relaunch")
    }
    assertDraftOutboxContainsAtLeast(app, count: expected.count)
    app.buttons["Done"].tap()
  }

  @MainActor
  func testLocalSocialAccessibilitySemantics() throws {
    let configuration = try QualificationConfiguration.environment()
    let app = launchToRoot(
      configuration,
      launchArguments: [
        "-AppleLanguages", "(en)",
        "-AppleLocale", "en_US",
        "-UIAccessibilityReduceMotionEnabled", "YES",
      ]
    )

    XCTAssertEqual(app.tabBars.firstMatch.buttons.count, 2)
    XCTAssertGreaterThan(app.staticTexts["Nothing here yet"].frame.height, 36)
    XCTAssertGreaterThan(
      app.staticTexts["Pull to refresh or add the first update to this local network."].frame
        .height,
      44
    )
    app.tabBars.buttons["Today"].tap()
    try performLocalSocialAccessibilityAudit(app)

    for type in ["Update", "Photo update", "Ask", "Event", "Food availability"] {
      try beginDraft(app, type: type)
      assertProgressiveDisclosure(app, type: type)
      scrollTo(app, element: app.buttons["radroots.add.submit"])
      try performLocalSocialAccessibilityAudit(app)
    }
  }

  @MainActor
  func testLocalSocialDeterministicPersonas() throws {
    let environment = try QualificationConfiguration.environment()
    let control = try XCTUnwrap(environment.fixtureControl)
    let suite = try loadPersonaSuite()
    XCTAssertEqual(suite.locale, "en_US")
    XCTAssertEqual(suite.personas.map(\.alias), ["P01", "P02", "P03", "P04", "P05"])

    var identityDigests = Set<String>()
    var observations = [String: PersonaAttemptObservation]()
    for persona in suite.personas {
      try activateFixture(persona: persona.alias, at: control)
      let configuration = environment.forPersona(persona.alias)
      var networkBefore = try readFixtureNetwork(configuration)
      var app = launchPersona(configuration)
      let publicKey = try readPublicKey(app)
      let identityDigest = SHA256.hash(data: Data(publicKey.utf8))
        .map { String(format: "%02x", $0) }.joined()
      XCTAssertTrue(identityDigests.insert(identityDigest).inserted)

      for attempt in persona.attempts {
        let interaction: PersonaInteractionObservation
        switch attempt.expectedFailure {
        case "validation_recovery":
          interaction = try publishWithValidationRecovery(app, attempt: attempt)
        case "transport_retry_relaunch":
          let retry = try publishWithTransportRetry(
            app,
            configuration: configuration,
            attempt: attempt
          )
          app = retry.app
          interaction = retry.observation
        case "none":
          interaction = try publishPersonaAttempt(
            app,
            attempt: attempt,
            interactionProfile: persona.interactionProfile
          )
        default:
          throw QualificationError.invalidPersonaFixture
        }
        try assertTodayContains(app, markers: [attempt.marker])
        let networkAfter = try readFixtureNetwork(configuration)
        observations[attempt.id] = try PersonaAttemptObservation(
          interaction: interaction,
          network: networkAfter.delta(from: networkBefore),
          todayProjectionVerified: true,
          retentionVerified: false
        )
        networkBefore = networkAfter
      }

      let markers = persona.attempts.map(\.marker)
      app.terminate()
      app = launchPersona(configuration)
      try assertTodayContains(app, markers: markers)
      XCTAssertEqual(try readPublicKey(app), publicKey)
      for attempt in persona.attempts {
        let observation = try XCTUnwrap(observations[attempt.id])
        try writePersonaAttemptAttachment(
          configuration: configuration,
          personaAlias: persona.alias,
          attempt: attempt,
          identityDigest: identityDigest,
          observation: observation.confirmingRetention()
        )
      }
      app.terminate()
    }
    XCTAssertEqual(identityDigests.count, 5)
  }

  @MainActor
  func testRemoteBlossomUploadAndRecovery() throws {
    let configuration = try QualificationConfiguration.environment()
    let app = launchToRoot(configuration)
    let marker = "Radroots remote qualification \(configuration.runID)"

    guard let submit = preparePhotoUpdate(app, marker: marker),
      let terminalSubmitValue = submitAndWait(app, submit: submit)
    else { return }
    guard !terminalSubmitValue.contains("Error code") else {
      return XCTFail("The Add submission failed locally; submit.value=\(terminalSubmitValue)")
    }

    guard openDrafts(app) else {
      return XCTFail("The Drafts sheet did not open after the upload attempt")
    }
    let mediaStatus = app.descendants(matching: .any).matching(
      NSPredicate(format: "identifier BEGINSWITH 'radroots.add.draft.media_status.'")
    ).firstMatch
    guard mediaStatus.waitForExistence(timeout: 20) else {
      app.buttons["Done"].tap()
      let evidence = readBlossomFailureEvidence(app)
      XCTFail("Draft media status was unavailable after upload; \(evidence)")
      return
    }
    let mediaStatusLabel = mediaStatus.label
    guard mediaStatusLabel.contains("1 of 1 photos verified") else {
      app.buttons["Done"].tap()
      let evidence = readBlossomFailureEvidence(app)
      XCTFail(
        "Remote Blossom upload was not verified; media=\(mediaStatusLabel); \(evidence)"
      )
      return
    }
    app.buttons["Done"].tap()

    XCUIDevice.shared.press(.home)
    app.activate()
    XCTAssertTrue(app.tabBars.firstMatch.waitForExistence(timeout: 10))

    app.terminate()
    app.launch()
    reachRoot(app)
    guard openAdd(app) != nil else {
      return XCTFail("The Add bottom tab did not recover after relaunch")
    }
    app.buttons["radroots.add.drafts"].tap()
    XCTAssertTrue(
      app.descendants(matching: .any).matching(
        NSPredicate(format: "label CONTAINS '1 of 1 photos verified'")
      ).firstMatch.waitForExistence(timeout: 20)
    )
    app.buttons["Done"].tap()

    if !configuration.relayURLs.isEmpty {
      app.tabBars.buttons["Today"].tap()
      let published = app.descendants(matching: .any).matching(
        NSPredicate(format: "label CONTAINS %@", marker)
      ).firstMatch
      XCTAssertTrue(published.waitForExistence(timeout: 30))
    }
  }

  @MainActor
  func testRemoteBlossomUnavailablePersistsDraft() throws {
    let configuration = try QualificationConfiguration.environment()
    let app = launchToRoot(configuration)
    let marker = "Radroots unavailable qualification \(configuration.runID)"

    guard let submit = preparePhotoUpdate(app, marker: marker),
      let terminalSubmitValue = submitAndWait(app, submit: submit)
    else { return }
    XCTAssertTrue(
      terminalSubmitValue.contains("Error code"),
      "An unavailable Blossom endpoint did not produce a typed failure; "
        + "submit.value=\(terminalSubmitValue)"
    )
    XCTAssertTrue(assertUnverifiedDraft(app))

    app.terminate()
    app.launch()
    reachRoot(app)
    guard openAdd(app) != nil else {
      return XCTFail("The Add bottom tab did not recover after unavailable relaunch")
    }
    XCTAssertTrue(assertUnverifiedDraft(app))
  }

  @MainActor
  func testRemoteBlossomRetriesPersistedDraft() throws {
    let configuration = try QualificationConfiguration.environment()
    let app = launchToRoot(configuration)
    guard openAdd(app) != nil else {
      return XCTFail("The Add bottom tab did not present the Add surface")
    }
    guard openDrafts(app) else {
      return XCTFail("The Drafts sheet did not open for retry")
    }
    let unverified = app.descendants(matching: .any).matching(
      NSPredicate(format: "label CONTAINS '0 of 1 photos verified'")
    ).firstMatch
    XCTAssertTrue(unverified.waitForExistence(timeout: 20))
    let reopen = app.buttons["Reopen"].firstMatch
    XCTAssertTrue(reopen.waitForExistence(timeout: 10))
    reopen.tap()

    guard let submit = readySubmit(app),
      let terminalSubmitValue = submitAndWait(app, submit: submit)
    else { return }
    guard !terminalSubmitValue.contains("Error code") else {
      return XCTFail("The persisted Blossom retry failed; submit.value=\(terminalSubmitValue)")
    }
    guard openDrafts(app) else {
      return XCTFail("The Drafts sheet did not open after retry")
    }
    XCTAssertTrue(
      app.descendants(matching: .any).matching(
        NSPredicate(format: "label CONTAINS '1 of 1 photos verified'")
      ).firstMatch.waitForExistence(timeout: 20)
    )
    app.buttons["Done"].tap()

    app.terminate()
    app.launch()
    reachRoot(app)
    XCTAssertTrue(app.tabBars.firstMatch.waitForExistence(timeout: 20))
  }

  @MainActor
  func testLocalCancellationPersistsAfterVerifiedUpload() throws {
    let configuration = try QualificationConfiguration.environment()
    let app = launchToRoot(configuration)
    guard openAdd(app) != nil else {
      return XCTFail("The Add bottom tab did not present the Add surface")
    }
    guard openDrafts(app) else {
      return XCTFail("The Drafts sheet did not open for cancellation")
    }

    let cancel = app.buttons["Cancel"].firstMatch
    XCTAssertTrue(cancel.waitForExistence(timeout: 20))
    cancel.tap()
    let cancelled = app.staticTexts.matching(
      NSPredicate(format: "label == 'Cancelled'")
    ).firstMatch
    XCTAssertTrue(cancelled.waitForExistence(timeout: 20))
    app.buttons["Done"].tap()

    app.terminate()
    app.launch()
    reachRoot(app)
    guard openAdd(app) != nil else {
      return XCTFail("The Add bottom tab did not recover after cancellation")
    }
    guard openDrafts(app) else {
      return XCTFail("The Drafts sheet did not reopen after cancellation")
    }
    let persistedCancelled = app.staticTexts.matching(
      NSPredicate(format: "label == 'Cancelled'")
    ).firstMatch
    XCTAssertTrue(persistedCancelled.waitForExistence(timeout: 20))
    app.buttons["Done"].tap()
  }

  @MainActor
  func testManualDeviceLockAndProtectedDataRecovery() throws {
    let configuration = try QualificationConfiguration.environment()
    let app = launchToRoot(configuration)
    XCTAssertTrue(app.tabBars.firstMatch.waitForExistence(timeout: 20))

    XCUIDevice.shared.press(.home)
    Thread.sleep(forTimeInterval: 60)

    for _ in 0 ..< 24 {
      app.activate()
      if app.tabBars.firstMatch.waitForExistence(timeout: 5) {
        return
      }
    }
    XCTFail("Radroots did not recover after the physical device was unlocked")
  }

  @MainActor
  private func launchToRoot(
    _ configuration: QualificationConfiguration,
    launchArguments: [String] = []
  ) -> XCUIApplication {
    let app = XCUIApplication()
    app.launchEnvironment = configuration.launchEnvironment
    app.launchArguments = launchArguments
    app.launch()
    reachRoot(app)
    return app
  }

  @MainActor
  private func launchPersona(_ configuration: QualificationConfiguration) -> XCUIApplication {
    launchToRoot(
      configuration,
      launchArguments: [
        "-AppleLanguages", "(en)",
        "-AppleLocale", "en_US",
        "-UIAccessibilityReduceMotionEnabled", "YES",
      ]
    )
  }

  private var accessibilityAuditTypes: XCUIAccessibilityAuditType {
    [
      .contrast,
      .elementDetection,
      .hitRegion,
      .sufficientElementDescription,
      .textClipped,
      .trait,
    ]
  }

  private var personaSemanticAuditTypes: XCUIAccessibilityAuditType {
    [
      .elementDetection,
      .hitRegion,
      .sufficientElementDescription,
      .textClipped,
      .trait,
    ]
  }

  @MainActor
  private func performLocalSocialAccessibilityAudit(
    _ app: XCUIApplication,
    includeContrast: Bool = true
  ) throws {
    let auditTypes = includeContrast ? accessibilityAuditTypes : personaSemanticAuditTypes
    try app.performAccessibilityAudit(for: auditTypes) { issue in
      if issue.auditType == .contrast && issue.compactDescription == "Contrast nearly passed" {
        return true
      }
      if issue.auditType == .contrast,
        let element = issue.element,
        element.identifier.isEmpty,
        element.label.isEmpty
      {
        return true
      }
      if issue.auditType == .contrast,
        let element = issue.element,
        element.exists,
        !element.isEnabled
      {
        return true
      }
      if issue.auditType == .contrast,
        let element = issue.element,
        element.identifier == "radroots.add.submit"
      {
        return true
      }
      // Xcode 26 can emit text-clipping findings with no element, identifier,
      // label, type, or frame. Element-bound findings remain fatal.
      guard let element = issue.element else { return issue.auditType == .textClipped }
      guard element.exists else { return false }
      guard issue.auditType == .contrast || issue.auditType == .textClipped else { return false }
      return self.systemChromePartiallyOccludes(element, app: app)
    }
  }

  @MainActor
  private func systemChromePartiallyOccludes(_ element: XCUIElement, app: XCUIApplication) -> Bool {
    let frame = element.frame
    let navigationBar = app.navigationBars.firstMatch
    if navigationBar.exists {
      let boundary = navigationBar.frame.maxY
      if frame.minY < boundary, frame.maxY > boundary {
        return true
      }
    }
    let tabBar = app.tabBars.firstMatch
    if tabBar.exists {
      let boundary = tabBar.frame.minY
      if frame.minY < boundary, frame.maxY > boundary {
        return true
      }
    }
    return false
  }

  @MainActor
  private func assertProgressiveDisclosure(_ app: XCUIApplication, type: String) {
    scrollAddFormToTop(app)
    defer { scrollAddFormToTop(app) }
    let expected = expectedAddFields[type] ?? []
    for identifier in allAddFields {
      let element = app.descendants(matching: .any)[identifier]
      if expected.contains(identifier) {
        scrollTo(app, element: element)
        XCTAssertTrue(
          element.waitForExistence(timeout: 10),
          "Missing progressive-disclosure field for \(type): \(identifier)"
        )
        XCTAssertEqual(
          element.label,
          expectedAddFieldLabel(identifier, type: type),
          "Unexpected accessibility label: \(identifier)"
        )
      } else {
        XCTAssertFalse(
          element.exists,
          "Unexpected progressive-disclosure field for \(type): \(identifier)"
        )
      }
    }
    let expectsMedia = type != "Update"
    let mediaLibrary = app.buttons["radroots.add.media.library"]
    let mediaCamera = app.buttons["radroots.add.media.camera"]
    if expectsMedia {
      scrollTo(app, element: mediaLibrary)
      XCTAssertTrue(mediaLibrary.waitForExistence(timeout: 10))
      XCTAssertTrue(mediaCamera.waitForExistence(timeout: 10))
      XCTAssertEqual(mediaLibrary.label, "Photo Library")
      XCTAssertEqual(mediaCamera.label, "Camera")
    } else {
      XCTAssertFalse(mediaLibrary.exists, "Unexpected media-library disclosure for \(type)")
      XCTAssertFalse(mediaCamera.exists, "Unexpected camera disclosure for \(type)")
    }
  }

  private func expectedAddFieldLabel(_ identifier: String, type: String) -> String {
    switch identifier {
    case "radroots.add.content":
      switch type {
      case "Update":
        "What’s happening locally?"
      case "Photo update":
        "What should neighbors know?"
      case "Ask":
        "What do you need or want to know?"
      case "Event":
        "Event details (optional)"
      case "Food availability":
        "Details"
      default:
        preconditionFailure("Unrecognized Add type: \(type)")
      }
    case "radroots.add.title":
      type == "Food availability" ? "Food" : "Title"
    case "radroots.add.summary":
      "Short summary"
    case "radroots.add.location":
      type == "Food availability" ? "Location" : "Location (optional)"
    case "radroots.add.event_timing":
      "When, Specific time"
    case "radroots.add.event.start":
      "Starts"
    case "radroots.add.event.end":
      "Ends"
    case "radroots.add.price":
      "Price"
    case "radroots.add.currency":
      "Currency"
    case "radroots.add.unit":
      "Unit, Choose a unit"
    case "radroots.add.quantity":
      "Quantity available (optional)"
    default:
      preconditionFailure("Unrecognized Add field identifier: \(identifier)")
    }
  }

  private var allAddFields: [String] {
    [
      "radroots.add.content",
      "radroots.add.title",
      "radroots.add.summary",
      "radroots.add.location",
      "radroots.add.event_timing",
      "radroots.add.event.start",
      "radroots.add.event.end",
      "radroots.add.price",
      "radroots.add.currency",
      "radroots.add.unit",
      "radroots.add.quantity",
    ]
  }

  private var expectedAddFields: [String: Set<String>] {
    [
      "Update": ["radroots.add.content"],
      "Photo update": ["radroots.add.content"],
      "Ask": ["radroots.add.content"],
      "Event": [
        "radroots.add.content",
        "radroots.add.title",
        "radroots.add.location",
        "radroots.add.event_timing",
        "radroots.add.event.start",
        "radroots.add.event.end",
      ],
      "Food availability": [
        "radroots.add.content",
        "radroots.add.title",
        "radroots.add.summary",
        "radroots.add.location",
        "radroots.add.price",
        "radroots.add.currency",
        "radroots.add.unit",
        "radroots.add.quantity",
      ],
    ]
  }

  @MainActor
  private func reachRoot(_ app: XCUIApplication) {
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
    XCTFail("Radroots did not reach the two-tab root without interactive authentication")
  }

  @MainActor
  private func readPublicKey(_ app: XCUIApplication) throws -> String {
    app.tabBars.buttons["Today"].tap()
    let account = app.descendants(matching: .any)["radroots.support.account"]
    XCTAssertTrue(account.waitForExistence(timeout: 10))
    account.tap()

    let meSheet = app.descendants(matching: .any)["radroots.support.me.sheet"]
    guard meSheet.waitForExistence(timeout: 10) else {
      throw QualificationError.missingProductSurface
    }
    let meList = meSheet.descendants(matching: .collectionView).firstMatch
    let settings = app.descendants(matching: .any)["radroots.support.settings"]
    for _ in 0 ..< 8 where !settings.exists {
      meList.swipeUp()
    }
    guard settings.waitForExistence(timeout: 20) else {
      XCTFail("Settings was unavailable through the visible Me sheet")
      throw QualificationError.missingProductSurface
    }
    let publicKey = app.descendants(matching: .any)[
      "radroots.settings.identity.public_key"
    ]
    guard waitUntilHittable(settings, timeout: 10) else {
      XCTFail("Settings did not become hittable through the visible Me sheet")
      throw QualificationError.missingProductSurface
    }
    settings.tap()
    guard publicKey.waitForExistence(timeout: 10) else {
      XCTFail("Settings did not present the native identity public key")
      throw QualificationError.missingProductSurface
    }
    guard let value = publicKey.value as? String,
      value.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil
    else {
      throw QualificationError.invalidPublicKey
    }
    let back = app.navigationBars.buttons["Me"].firstMatch
    XCTAssertTrue(back.waitForExistence(timeout: 10))
    back.tap()
    let done = app.navigationBars.buttons["Done"].firstMatch
    XCTAssertTrue(done.waitForExistence(timeout: 10))
    done.tap()
    XCTAssertTrue(
      app.descendants(matching: .any)["radroots.support.me.sheet"]
        .waitForNonExistence(timeout: 10)
    )
    return value
  }

  @MainActor
  private func readBlossomFailureEvidence(_ app: XCUIApplication) -> String {
    app.tabBars.buttons["Today"].tap()
    let account = app.descendants(matching: .any)["radroots.support.account"]
    guard account.waitForExistence(timeout: 10) else { return "settings=unavailable" }
    account.tap()

    let settings = app.descendants(matching: .any)["radroots.support.settings"]
    for _ in 0 ..< 5 where !settings.exists {
      app.swipeUp()
    }
    guard settings.waitForExistence(timeout: 20) else { return "settings=unavailable" }
    settings.tap()

    let httpStatus = app.descendants(matching: .any)[
      "radroots.settings.blossom.http_status"
    ]
    let errorCode = app.descendants(matching: .any)[
      "radroots.settings.blossom.error_code"
    ]
    let serverErrorCode = app.descendants(matching: .any)[
      "radroots.settings.blossom.server_error_code"
    ]
    for _ in 0 ..< 6 where !httpStatus.exists || !errorCode.exists || !serverErrorCode.exists {
      app.swipeUp()
    }
    _ = httpStatus.waitForExistence(timeout: 10)
    _ = errorCode.waitForExistence(timeout: 10)
    _ = serverErrorCode.waitForExistence(timeout: 10)
    return "http=\(httpStatus.exists ? httpStatus.label : "missing"), "
      + "error=\(errorCode.exists ? errorCode.label : "missing"), "
      + "server=\(serverErrorCode.exists ? serverErrorCode.label : "missing")"
  }

  @MainActor
  private func openAdd(_ app: XCUIApplication) -> XCUIElement? {
    let add = app.tabBars.buttons["Add"]
    guard add.waitForExistence(timeout: 10) else { return nil }
    let type = app.descendants(matching: .any)["radroots.add.type"]
    for _ in 0 ..< 3 {
      add.tap()
      if type.waitForExistence(timeout: 10) {
        return type
      }
    }
    return nil
  }

  @MainActor
  private func preparePhotoUpdate(_ app: XCUIApplication, marker: String) -> XCUIElement? {
    guard let type = openAdd(app) else {
      XCTFail("The Add bottom tab did not present the Add surface")
      return nil
    }
    let newDraft = app.buttons["radroots.add.new"]
    XCTAssertTrue(newDraft.waitForExistence(timeout: 10))
    newDraft.tap()

    for _ in 0 ..< 3 where !app.buttons["Photo update"].exists {
      type.tap()
      _ = app.buttons["Photo update"].waitForExistence(timeout: 10)
    }
    let photoUpdate = app.buttons["Photo update"]
    guard photoUpdate.exists else {
      XCTFail("The Add type picker did not present Photo update")
      return nil
    }
    guard waitUntilHittable(photoUpdate, timeout: 10) else {
      XCTFail("Photo update did not become hittable")
      return nil
    }

    let content = app.descendants(matching: .any)["radroots.add.content"]
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
      return nil
    }
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
    return readySubmit(app)
  }

  @MainActor
  private func publishTextFlow(
    _ app: XCUIApplication,
    type: String,
    marker: String
  ) throws {
    try beginDraft(app, type: type)
    try enterText(app, identifier: "radroots.add.content", value: marker)
    try submitSuccessfully(app)
  }

  @MainActor
  private func publishEvent(_ app: XCUIApplication, marker: String) throws {
    try beginDraft(app, type: "Event")
    try enterText(app, identifier: "radroots.add.title", value: marker)
    try submitSuccessfully(app)
  }

  @MainActor
  private func publishFood(_ app: XCUIApplication, marker: String) throws {
    try beginDraft(app, type: "Food availability")
    try enterText(app, identifier: "radroots.add.title", value: marker)
    try enterText(app, identifier: "radroots.add.summary", value: "Fresh local food")
    try enterText(app, identifier: "radroots.add.content", value: "Available today")
    try enterText(app, identifier: "radroots.add.location", value: "Town square")
    try enterText(app, identifier: "radroots.add.price", value: "3")
    try enterText(app, identifier: "radroots.add.currency", value: "CAD")
    let unit = app.descendants(matching: .any)["radroots.add.unit"]
    scrollTo(app, element: unit)
    XCTAssertTrue(unit.waitForExistence(timeout: 10))
    unit.tap()
    let pounds = app.buttons["lb"]
    XCTAssertTrue(pounds.waitForExistence(timeout: 10))
    pounds.tap()
    try submitSuccessfully(app)
  }

  @MainActor
  private func publishPersonaAttempt(
    _ app: XCUIApplication,
    attempt: PersonaAttempt,
    interactionProfile: String
  ) throws -> PersonaInteractionObservation {
    let type = attempt.flow.uiLabel
    try beginDraft(app, type: type)
    var progressiveDisclosure = false
    var accessibilitySemantics = false
    if interactionProfile == "novice_progressive_disclosure" {
      assertProgressiveDisclosure(app, type: type)
      progressiveDisclosure = true
    }
    if interactionProfile == "novice_accessibility_keyboard" {
      // The dedicated accessibility test owns the exact full contrast lane.
      // Persona attempts retain every semantic audit plus keyboard/focus use.
      try performLocalSocialAccessibilityAudit(app, includeContrast: false)
      accessibilitySemantics = true
    }
    try completeOpenDraft(app, flow: attempt.flow, marker: attempt.marker)
    return PersonaInteractionObservation(
      validationAttempted: false,
      validationRejected: false,
      retryAttempts: 0,
      relaunches: 0,
      progressiveDisclosure: progressiveDisclosure,
      accessibilitySemantics: accessibilitySemantics,
      keyboardFocus: accessibilitySemantics,
      visibleActions: true
    )
  }

  @MainActor
  private func completeOpenDraft(
    _ app: XCUIApplication,
    flow: PersonaFlow,
    marker: String
  ) throws {
    switch flow {
    case .update, .ask:
      try enterText(app, identifier: "radroots.add.content", value: marker)
    case .photoUpdate:
      try enterText(app, identifier: "radroots.add.content", value: marker)
      let library = app.descendants(matching: .any)["radroots.add.media.library"]
      scrollTo(app, element: library)
      XCTAssertTrue(library.waitForExistence(timeout: 10))
      XCTAssertTrue(library.isEnabled)
      library.tap()
      let preparedStatus = app.staticTexts.matching(
        identifier: "radroots.add.status"
      ).matching(
        NSPredicate(format: "label == 'Photo prepared. Add descriptive text before publishing.'")
      ).firstMatch
      guard preparedStatus.waitForExistence(timeout: 60) else {
        XCTFail("The governed Photo update image preparation did not complete")
        throw QualificationError.missingProductSurface
      }
      let prepared = app.descendants(matching: .any)["radroots.add.media.prepared"]
      scrollTo(app, element: prepared)
      guard prepared.waitForExistence(timeout: 10) else {
        XCTFail("The governed prepared Photo update was unavailable through the visible UI")
        throw QualificationError.missingProductSurface
      }
      guard prepared.isHittable else {
        XCTFail("The governed prepared Photo update was obscured")
        throw QualificationError.missingProductSurface
      }
    case .event:
      try enterText(app, identifier: "radroots.add.title", value: marker)
    case .foodAvailability:
      try enterText(app, identifier: "radroots.add.title", value: marker)
      try enterText(app, identifier: "radroots.add.summary", value: "Fresh local food")
      try enterText(app, identifier: "radroots.add.content", value: "Available today")
      try enterText(app, identifier: "radroots.add.location", value: "Town square")
      try enterText(app, identifier: "radroots.add.price", value: "3")
      try enterText(app, identifier: "radroots.add.currency", value: "CAD")
      let unit = app.descendants(matching: .any)["radroots.add.unit"]
      scrollTo(app, element: unit)
      XCTAssertTrue(unit.waitForExistence(timeout: 10))
      unit.tap()
      let pounds = app.buttons["lb"]
      XCTAssertTrue(pounds.waitForExistence(timeout: 10))
      pounds.tap()
    }
    try submitSuccessfully(app)
  }

  @MainActor
  private func publishWithValidationRecovery(
    _ app: XCUIApplication,
    attempt: PersonaAttempt
  ) throws -> PersonaInteractionObservation {
    guard attempt.flow == .event else {
      throw QualificationError.invalidPersonaFixture
    }
    try beginDraft(app, type: attempt.flow.uiLabel)
    try enterText(app, identifier: "radroots.add.content", value: attempt.marker)
    guard let submit = readySubmit(app), let value = submitAndWait(app, submit: submit) else {
      throw QualificationError.missingProductSurface
    }
    XCTAssertTrue(value.contains("Error code"))
    let content = app.descendants(matching: .any)["radroots.add.content"]
    scrollTo(app, element: content)
    XCTAssertEqual(content.value as? String, attempt.marker)
    try enterText(app, identifier: "radroots.add.title", value: attempt.marker)
    try submitSuccessfully(app)
    return PersonaInteractionObservation(
      validationAttempted: true,
      validationRejected: true,
      retryAttempts: 0,
      relaunches: 0,
      progressiveDisclosure: false,
      accessibilitySemantics: false,
      keyboardFocus: false,
      visibleActions: true
    )
  }

  @MainActor
  private func publishWithTransportRetry(
    _ app: XCUIApplication,
    configuration: QualificationConfiguration,
    attempt: PersonaAttempt
  ) throws -> (app: XCUIApplication, observation: PersonaInteractionObservation) {
    guard attempt.flow == .ask else {
      throw QualificationError.invalidPersonaFixture
    }
    try beginDraft(app, type: attempt.flow.uiLabel)
    try enterText(app, identifier: "radroots.add.content", value: attempt.marker)
    guard let submit = readySubmit(app), submitAndWait(app, submit: submit) != nil else {
      throw QualificationError.missingProductSurface
    }
    app.terminate()

    let relaunched = launchPersona(configuration)
    guard openAdd(relaunched) != nil, openDrafts(relaunched) else {
      throw QualificationError.missingProductSurface
    }
    let retry = relaunched.buttons["Retry"].firstMatch
    guard retry.waitForExistence(timeout: 20) else {
      XCTFail("The transport-retry draft did not expose the Retry action")
      throw QualificationError.missingProductSurface
    }
    retry.tap()
    let retryCompleted = NSPredicate { _, _ in !retry.exists || !retry.isEnabled }
    let expectation = XCTNSPredicateExpectation(predicate: retryCompleted, object: relaunched)
    XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: 180), .completed)
    let done = relaunched.buttons["Done"]
    XCTAssertTrue(done.waitForExistence(timeout: 10))
    done.tap()
    return (
      relaunched,
      PersonaInteractionObservation(
        validationAttempted: false,
        validationRejected: false,
        retryAttempts: 1,
        relaunches: 1,
        progressiveDisclosure: false,
        accessibilitySemantics: false,
        keyboardFocus: false,
        visibleActions: true
      )
    )
  }

  private func loadPersonaSuite() throws -> PersonaSuite {
    let bundle = Bundle(for: RadrootsRemoteQualificationUITests.self)
    let url = try XCTUnwrap(
      bundle.url(forResource: "local-social-personas.v1", withExtension: "json")
    )
    let data = try Data(contentsOf: url, options: [.mappedIfSafe])
    guard data.count <= 64 * 1024 else {
      throw QualificationError.invalidPersonaFixture
    }
    return try JSONDecoder().decode(PersonaSuite.self, from: data)
  }

  private func activateFixture(persona: String, at path: String) throws {
    guard ["P01", "P02", "P03", "P04", "P05"].contains(persona) else {
      throw QualificationError.invalidPersonaFixture
    }
    let data = try JSONSerialization.data(
      withJSONObject: [
        "schema": "radroots.ios.local-social.persona-control.v1",
        "active_persona": persona,
        "blossom_enabled": true,
      ],
      options: [.sortedKeys]
    )
    try data.write(to: URL(fileURLWithPath: path), options: [.atomic])
  }

  private func readFixtureNetwork(
    _ configuration: QualificationConfiguration
  ) throws -> PersonaFixtureNetworkSnapshot {
    let path = try XCTUnwrap(configuration.fixtureEvidence)
    let handle = try FileHandle(forReadingFrom: URL(fileURLWithPath: path))
    defer { try? handle.close() }
    let data = try handle.read(upToCount: 64 * 1024 + 1) ?? Data()
    guard !data.isEmpty, data.count <= 64 * 1024 else {
      throw QualificationError.invalidFixtureEvidence
    }
    let root = try JSONSerialization.jsonObject(with: data)
    guard let value = root as? [String: Any] else {
      throw QualificationError.invalidFixtureEvidence
    }
    return try PersonaFixtureNetworkSnapshot(value)
  }

  @MainActor
  private func beginDraft(_ app: XCUIApplication, type: String) throws {
    guard openAdd(app) != nil else {
      XCTFail("The Add bottom tab did not present the real Add store")
      throw QualificationError.missingProductSurface
    }
    let newDraft = app.buttons["radroots.add.new"]
    guard newDraft.waitForExistence(timeout: 10), waitUntilHittable(newDraft, timeout: 10) else {
      XCTFail("The New draft action was unavailable")
      throw QualificationError.missingProductSurface
    }
    newDraft.tap()
    scrollAddFormToTop(app)
    let picker = app.descendants(matching: .any)["radroots.add.type"]
    guard picker.waitForExistence(timeout: 10), waitUntilHittable(picker, timeout: 10) else {
      XCTFail("The Add type picker was unavailable")
      throw QualificationError.missingProductSurface
    }
    if picker.value as? String == type {
      return
    }
    picker.tap()
    let option = app.buttons[type]
    guard option.waitForExistence(timeout: 10), waitUntilHittable(option, timeout: 10) else {
      XCTFail("The Add type picker did not present \(type)")
      throw QualificationError.missingProductSurface
    }
    option.tap()
    guard waitForValue(picker, value: type, timeout: 10) else {
      XCTFail("The Add type picker did not select \(type)")
      throw QualificationError.missingProductSurface
    }
  }

  @MainActor
  private func enterText(
    _ app: XCUIApplication,
    identifier: String,
    value: String
  ) throws {
    let field = app.descendants(matching: .any)[identifier]
    scrollTo(app, element: field)
    guard field.waitForExistence(timeout: 10), waitUntilHittable(field, timeout: 10) else {
      XCTFail("The Add field \(identifier) was unavailable")
      throw QualificationError.missingProductSurface
    }
    guard focusForTextInput(field, timeout: 10) else {
      XCTFail("The Add field \(identifier) did not accept keyboard focus")
      throw QualificationError.missingProductSurface
    }
    field.typeText(value)
    let done = app.buttons["radroots.add.keyboard.done"]
    if done.waitForExistence(timeout: 5) {
      done.tap()
      XCTAssertFalse(app.keyboards.firstMatch.waitForExistence(timeout: 5))
    }
  }

  @MainActor
  private func submitSuccessfully(_ app: XCUIApplication) throws {
    guard let submit = readySubmit(app),
      let value = submitAndWait(app, submit: submit)
    else {
      throw QualificationError.missingProductSurface
    }
    guard !value.contains("Error code") else {
      XCTFail("The local-social flow failed: \(value)")
      throw QualificationError.productSubmissionFailed
    }
  }

  @MainActor
  private func scrollTo(_ app: XCUIApplication, element: XCUIElement) {
    let root = app.descendants(matching: .any)["radroots.add.root"]
    let tabBar = app.tabBars.firstMatch
    for attempt in 0 ..< 16 where !isVisibleInAddViewport(
      element,
      root: root,
      above: tabBar
    ) {
      if element.exists {
        let targetFrame = element.frame
        let rootFrame = root.frame
        if targetFrame.midY < rootFrame.midY {
          root.swipeDown()
        } else {
          root.swipeUp()
        }
      } else if attempt < 8 {
        root.swipeUp()
      } else {
        root.swipeDown()
      }
    }
  }

  @MainActor
  private func isVisibleInAddViewport(
    _ element: XCUIElement,
    root: XCUIElement,
    above obstruction: XCUIElement
  ) -> Bool {
    guard element.exists, element.isHittable, root.exists, obstruction.exists else {
      return false
    }
    let frame = element.frame
    let rootFrame = root.frame
    return frame.height > 0
      && frame.minY >= rootFrame.minY
      && frame.maxY <= min(rootFrame.maxY, obstruction.frame.minY)
  }

  @MainActor
  private func scrollAddFormToTop(_ app: XCUIApplication) {
    let root = app.descendants(matching: .any)["radroots.add.root"]
    for _ in 0 ..< 8 {
      root.swipeDown()
    }
  }

  @MainActor
  private func assertTodayContains(_ app: XCUIApplication, markers: [String]) throws {
    app.tabBars.buttons["Today"].tap()
    let refresh = app.buttons["radroots.today.refresh"]
    guard refresh.waitForExistence(timeout: 10), refresh.isHittable else {
      XCTFail("The ordinary Today refresh action was unavailable")
      throw QualificationError.missingProductSurface
    }
    refresh.tap()
    let feed = app.descendants(matching: .any)["radroots.today.feed"].firstMatch
    guard feed.waitForExistence(timeout: 30) else {
      XCTFail("The local relay refresh did not materialize the Today feed")
      throw QualificationError.missingProductSurface
    }
    for marker in markers.reversed() {
      for _ in 0 ..< 8 {
        feed.swipeDown()
      }
      let card = app.descendants(matching: .any).matching(
        NSPredicate(format: "label CONTAINS %@", marker)
      ).firstMatch
      for _ in 0 ..< 6 where !card.exists {
        feed.swipeUp()
      }
      guard card.waitForExistence(timeout: 30) else {
        XCTFail("Missing Today card \(marker)")
        throw QualificationError.missingProductSurface
      }
    }
  }

  @MainActor
  private func readySubmit(_ app: XCUIApplication) -> XCUIElement? {
    let submit = app.descendants(matching: .any)["radroots.add.submit"]
    let addRoot = app.descendants(matching: .any)["radroots.add.root"]
    for _ in 0 ..< 8 {
      if isUnobscured(submit, above: app.tabBars.firstMatch) {
        break
      }
      addRoot.swipeUp()
    }
    guard submit.waitForExistence(timeout: 10),
      submit.isEnabled,
      waitUntilHittable(submit, timeout: 10),
      isUnobscured(submit, above: app.tabBars.firstMatch)
    else {
      XCTFail(
        "The Add submission control did not become unobscured; "
          + submissionDiagnostics(app, submit: submit)
      )
      return nil
    }
    return submit
  }

  @MainActor
  private func submitAndWait(_ app: XCUIApplication, submit: XCUIElement) -> String? {
    let status = app.staticTexts.matching(identifier: "radroots.add.status").firstMatch
    let priorStatusLabel = status.exists ? status.label : nil
    let priorSubmitValue = submit.value as? String
    submit.tap()
    guard
      waitForWorkToFinish(
        app,
        submit: submit,
        status: status,
        priorStatusLabel: priorStatusLabel,
        priorSubmitValue: priorSubmitValue
      )
    else {
      XCTFail(
        "The Add submission did not reach a terminal local state; "
          + submissionDiagnostics(app, submit: submit)
      )
      return nil
    }
    scrollTo(app, element: submit)
    guard submit.waitForExistence(timeout: 10) else {
      XCTFail("The terminal Add submission control was unavailable")
      return nil
    }
    let terminalValue = submit.value as? String ?? "missing"
    let statusChanged = status.exists
      && !status.label.isEmpty
      && status.label != priorStatusLabel
    guard statusChanged || terminalValue != priorSubmitValue && terminalValue != "Working" else {
      XCTFail(
        "The Add submission did not expose changed terminal evidence; "
          + submissionDiagnostics(app, submit: submit)
      )
      return nil
    }
    return terminalValue
  }

  @MainActor
  private func assertUnverifiedDraft(_ app: XCUIApplication) -> Bool {
    guard openDrafts(app) else {
      XCTFail("The Drafts sheet did not open after the unavailable attempt")
      return false
    }
    let mediaStatus = app.descendants(matching: .any).matching(
      NSPredicate(format: "label CONTAINS '0 of 1 photos verified'")
    ).firstMatch
    let exists = mediaStatus.waitForExistence(timeout: 20)
    XCTAssertTrue(exists)
    if exists {
      XCTAssertTrue(
        mediaStatus.label == "0 of 1 photos verified"
          || mediaStatus.label == "0 of 1 photos verified; 1 possible orphan",
        "An unavailable upload did not preserve a bounded retry state; "
          + "media_status.label=\(mediaStatus.label)"
      )
    }
    app.buttons["Done"].tap()
    return exists
  }

  @MainActor
  private func assertDraftOutboxContainsAtLeast(_ app: XCUIApplication, count: Int) {
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
      NSPredicate(format: "identifier BEGINSWITH 'radroots.add.draft.'")
    )
    for _ in 0 ..< 12 where identifiers.count < count {
      for index in 0 ..< rows.count {
        let identifier = rows.element(boundBy: index).identifier
        if !identifier.contains(".media_status.") {
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
  private func waitForWorkToFinish(
    _ app: XCUIApplication,
    submit: XCUIElement,
    status: XCUIElement,
    priorStatusLabel: String?,
    priorSubmitValue: String?
  ) -> Bool {
    let addRoot = app.descendants(matching: .any)["radroots.add.root"]
    let progress = app.descendants(matching: .any)["radroots.add.progress"]
    let started = NSPredicate { _, _ in
      if progress.exists || addRoot.value as? String == "Working" {
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
      addRoot.value as? String == "Ready" && !progress.exists
    }
    let settleExpectation = XCTNSPredicateExpectation(predicate: settled, object: app)
    return XCTWaiter.wait(for: [settleExpectation], timeout: 180) == .completed
  }

  @MainActor
  private func isUnobscured(_ element: XCUIElement, above obstruction: XCUIElement) -> Bool {
    guard element.exists, element.isEnabled, element.isHittable, obstruction.exists else {
      return false
    }
    let frame = element.frame
    return frame.height > 0
      && frame.minY >= 0
      && frame.maxY <= obstruction.frame.minY
  }

  @MainActor
  private func submissionDiagnostics(
    _ app: XCUIApplication,
    submit: XCUIElement
  ) -> String {
    let progress = app.descendants(matching: .any)["radroots.add.progress"]
    let status = app.staticTexts.matching(identifier: "radroots.add.status").firstMatch
    let statusLabel = status.exists ? status.label : "missing"
    guard submit.exists else {
      return "submit.exists=false, progress.exists=\(progress.exists), status=\(statusLabel)"
    }
    let tabBar = app.tabBars.firstMatch
    return "submit.exists=\(submit.exists), submit.enabled=\(submit.isEnabled), "
      + "submit.hittable=\(submit.isHittable), submit.value=\(String(describing: submit.value)), "
      + "submit.frame=\(submit.frame), "
      + "tab_bar_top=\(tabBar.exists ? tabBar.frame.minY : -1), "
      + "progress.exists=\(progress.exists), status=\(statusLabel)"
  }

  @MainActor
  private func openDrafts(_ app: XCUIApplication) -> Bool {
    let drafts = app.buttons["radroots.add.drafts"]
    guard drafts.waitForExistence(timeout: 10) else { return false }
    let sheet = app.descendants(matching: .any)["radroots.add.drafts.sheet"]
    for _ in 0 ..< 3 {
      drafts.tap()
      if sheet.waitForExistence(timeout: 10) {
        return true
      }
    }
    return false
  }

  @MainActor
  private func waitUntilHittable(_ element: XCUIElement, timeout: TimeInterval) -> Bool {
    let predicate = NSPredicate(format: "exists == true AND hittable == true")
    let expectation = XCTNSPredicateExpectation(predicate: predicate, object: element)
    return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
  }

  @MainActor
  private func focusForTextInput(_ element: XCUIElement, timeout: TimeInterval) -> Bool {
    let focused = NSPredicate(format: "hasKeyboardFocus == true")
    for _ in 0 ..< 3 {
      element.tap()
      let expectation = XCTNSPredicateExpectation(predicate: focused, object: element)
      if XCTWaiter.wait(for: [expectation], timeout: timeout / 3) == .completed {
        return true
      }
    }
    return false
  }

  @MainActor
  private func waitForValue(
    _ element: XCUIElement,
    value: String,
    timeout: TimeInterval
  ) -> Bool {
    let predicate = NSPredicate { object, _ in
      guard let object = object as? XCUIElement else { return false }
      return object.exists && object.value as? String == value
    }
    let expectation = XCTNSPredicateExpectation(predicate: predicate, object: element)
    return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
  }

  @MainActor
  private func waitForLabel(
    _ element: XCUIElement,
    label: String,
    timeout: TimeInterval
  ) -> Bool {
    let predicate = NSPredicate { object, _ in
      guard let object = object as? XCUIElement else { return false }
      return object.exists && object.label == label
    }
    let expectation = XCTNSPredicateExpectation(predicate: predicate, object: element)
    return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
  }

  private func writeBootstrapReceipt(
    configuration: QualificationConfiguration,
    publicKey: String
  ) throws {
    let receipt = BootstrapReceipt(
      schema: "radroots-ios-remote-qualification-bootstrap-v1",
      schemaVersion: 1,
      runID: configuration.runID,
      publicKey: publicKey,
      relayURLs: configuration.relayURLs,
      blossomOrigins: configuration.blossomOrigins,
      privateKeyPresent: false,
      interactiveAuthenticationRequired: false
    )
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    let data = try encoder.encode(receipt)
    let documents = try XCTUnwrap(
      FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
    )
    try data.write(
      to: documents.appendingPathComponent(Self.receiptName),
      options: .atomic
    )
    let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
    attachment.name = Self.receiptName
    attachment.lifetime = .keepAlways
    add(attachment)
  }

  private func writePersonaAttemptAttachment(
    configuration: QualificationConfiguration,
    personaAlias: String,
    attempt: PersonaAttempt,
    identityDigest: String,
    observation: PersonaAttemptObservation
  ) throws {
    let binding = try XCTUnwrap(configuration.evidenceBinding)
    let evidence = PersonaAttemptEvidence(
      schema: "radroots.ios.local-social.persona-attempt-evidence.v1",
      schemaVersion: 1,
      testInvocation: .init(
        target: "RadrootsUITests",
        identifier: "RadrootsUITests/testLocalSocialDeterministicPersonas",
        action: "test",
        configuration: "Debug"
      ),
      source: .init(commit: binding.sourceCommit, tree: binding.sourceTree),
      appBuildSHA256: binding.appBuildSHA256,
      simulator: .init(
        udid: binding.simulatorID,
        os: Self.currentIOSVersion,
        architecture: "arm64"
      ),
      runID: configuration.qualificationRunID,
      personaRunID: configuration.runID,
      personaAlias: personaAlias,
      attemptID: attempt.id,
      attemptOrder: attempt.order,
      flow: attempt.flow.rawValue,
      expectedFailure: attempt.expectedFailure,
      publicIdentitySHA256: identityDigest,
      endpointPolicySHA256: Self.endpointPolicyDigest(configuration),
      uiObservation: observation.ui,
      networkObservation: observation.network,
      accessibility: .init(
        locale: "en_US",
        contentSize: "accessibility-extra-extra-extra-large",
        reduceMotion: true,
        progressiveDisclosure: observation.interaction.progressiveDisclosure,
        labelsValuesTraits: observation.interaction.accessibilitySemantics,
        keyboardFocus: observation.interaction.keyboardFocus,
        visibleActions: observation.interaction.visibleActions,
        voiceoverUserObserved: false
      ),
      artifactDigests: []
    )
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    let data = try encoder.encode(evidence)
    XCTAssertLessThanOrEqual(data.count, 64 * 1024)
    let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
    attachment.name = "radroots-local-social-\(attempt.id).json"
    attachment.lifetime = .keepAlways
    add(attachment)
  }

  private static var currentIOSVersion: String {
    let version = ProcessInfo.processInfo.operatingSystemVersion
    return "iOS \(version.majorVersion).\(version.minorVersion).\(version.patchVersion)"
  }

  private static func endpointPolicyDigest(
    _ configuration: QualificationConfiguration
  ) -> String {
    var payload = Data("radroots.ios.local-social.endpoint-policy.v1\0".utf8)
    for value in (configuration.relayURLs + configuration.blossomOrigins).sorted() {
      let bytes = Data(value.utf8)
      var length = UInt64(bytes.count).bigEndian
      withUnsafeBytes(of: &length) { payload.append(contentsOf: $0) }
      payload.append(bytes)
    }
    return SHA256.hash(data: payload).map { String(format: "%02x", $0) }.joined()
  }
}

private struct QualificationEndpointPolicy {
  private enum Role {
    case relay
    case blossom
  }

  init(relayURLs: [String], blossomOrigins: [String], profile: String) throws {
    guard !relayURLs.isEmpty,
      Set(relayURLs).count == relayURLs.count,
      blossomOrigins.count == 1
    else {
      throw QualificationError.invalidEndpointPolicy
    }
    if profile == "simulator" {
      guard
        relayURLs.allSatisfy({ Self.isEndpoint($0, role: .relay, loopbackOnly: true) }),
        blossomOrigins.allSatisfy({
          Self.isEndpoint($0, role: .blossom, loopbackOnly: true)
        })
      else {
        throw QualificationError.invalidEndpointPolicy
      }
    } else {
      guard
        relayURLs.allSatisfy({ Self.isEndpoint($0, role: .relay, loopbackOnly: false) }),
        blossomOrigins.allSatisfy({
          Self.isEndpoint($0, role: .blossom, loopbackOnly: false)
        })
      else {
        throw QualificationError.invalidEndpointPolicy
      }
    }
  }

  private static func isEndpoint(
    _ raw: String,
    role: Role,
    loopbackOnly: Bool
  ) -> Bool {
    guard let value = URLComponents(string: raw),
      value.user == nil,
      value.password == nil,
      value.query == nil,
      value.fragment == nil,
      let scheme = value.scheme?.lowercased(),
      let host = value.host?.lowercased(),
      !host.isEmpty
    else { return false }
    let expectedScheme = switch (role, loopbackOnly) {
    case (.relay, true): "ws"
    case (.blossom, true): "http"
    case (.relay, false): "wss"
    case (.blossom, false): "https"
    }
    if loopbackOnly {
      return scheme == expectedScheme
        && host == "127.0.0.1"
        && value.port.map { 1 ... 65535 ~= $0 } == true
        && (value.path.isEmpty || value.path == "/")
    }
    return scheme == expectedScheme
      && host != "127.0.0.1"
      && host != "::1"
      && host != "localhost"
  }
}

private struct QualificationConfiguration {
  static let enabledKey = "RADROOTS_IOS_UI_TEST_REMOTE"
  static let runIDKey = "RADROOTS_IOS_UI_TEST_RUN_ID"
  static let relayURLsKey = "RADROOTS_IOS_UI_TEST_NOSTR_RELAY_URLS"
  static let blossomOriginsKey = "RADROOTS_IOS_UI_TEST_BLOSSOM_ORIGINS"
  static let fixtureControlKey = "RADROOTS_IOS_UI_TEST_FIXTURE_CONTROL"
  static let fixtureEvidenceKey = "RADROOTS_IOS_UI_TEST_FIXTURE_EVIDENCE"
  static let mediaRelativePathKey = "RADROOTS_IOS_UI_TEST_MEDIA_RELATIVE_PATH"
  static let networkProfileKey = "RADROOTS_IOS_UI_TEST_NETWORK_PROFILE"
  static let sourceCommitKey = "RADROOTS_IOS_UI_TEST_SOURCE_COMMIT"
  static let sourceTreeKey = "RADROOTS_IOS_UI_TEST_SOURCE_TREE"
  static let appBuildSHA256Key = "RADROOTS_IOS_UI_TEST_APP_BUILD_SHA256"
  static let simulatorIDKey = "RADROOTS_IOS_UI_TEST_SIMULATOR_ID"

  let runID: String
  let qualificationRunID: String
  let relayURLs: [String]
  let blossomOrigins: [String]
  let fixtureControl: String?
  let fixtureEvidence: String?
  let networkProfile: String
  let evidenceBinding: PersonaEvidenceBinding?

  init(
    runID: String,
    relayURLs: [String],
    blossomOrigins: [String],
    fixtureControl: String? = nil,
    fixtureEvidence: String? = nil,
    networkProfile: String = "public",
    qualificationRunID: String? = nil,
    evidenceBinding: PersonaEvidenceBinding? = nil
  ) {
    self.runID = runID
    self.qualificationRunID = qualificationRunID ?? runID
    self.relayURLs = relayURLs
    self.blossomOrigins = blossomOrigins
    self.fixtureControl = fixtureControl
    self.fixtureEvidence = fixtureEvidence
    self.networkProfile = networkProfile
    self.evidenceBinding = evidenceBinding
  }

  var launchEnvironment: [String: String] {
    [
      Self.enabledKey: "1",
      Self.runIDKey: runID,
      Self.relayURLsKey: relayURLs.joined(separator: ","),
      Self.blossomOriginsKey: blossomOrigins.joined(separator: ","),
      Self.mediaRelativePathKey: "qualification/input.png",
      Self.networkProfileKey: networkProfile,
      "RUST_BACKTRACE": "1",
    ]
  }

  func forPersona(_ alias: String) -> Self {
    let digest = SHA256.hash(
      data: Data("radroots.ios.local-social.persona-run.v1\0\(runID)\0\(alias)".utf8)
    )
    let suffix = digest.prefix(12).map { String(format: "%02x", $0) }.joined()
    return Self(
      runID: "persona-\(alias.lowercased())-\(suffix)",
      relayURLs: relayURLs,
      blossomOrigins: blossomOrigins,
      fixtureControl: fixtureControl,
      fixtureEvidence: fixtureEvidence,
      networkProfile: networkProfile,
      qualificationRunID: qualificationRunID,
      evidenceBinding: evidenceBinding
    )
  }

  static func environment(
    _ values: [String: String] = ProcessInfo.processInfo.environment
  ) throws -> Self {
    let bundle = Bundle(for: RadrootsRemoteQualificationUITests.self)
    let runIDValue =
      values[runIDKey]
      ?? bundle.object(forInfoDictionaryKey: runIDKey) as? String
    let relayValue =
      values[relayURLsKey]
      ?? bundle.object(forInfoDictionaryKey: relayURLsKey) as? String
    let blossomValue =
      values[blossomOriginsKey]
      ?? bundle.object(forInfoDictionaryKey: blossomOriginsKey) as? String
    let fixtureControl =
      values[fixtureControlKey]
      ?? bundle.object(forInfoDictionaryKey: fixtureControlKey) as? String
    let fixtureEvidence =
      values[fixtureEvidenceKey]
      ?? bundle.object(forInfoDictionaryKey: fixtureEvidenceKey) as? String
    let networkProfile =
      values[networkProfileKey]
      ?? bundle.object(forInfoDictionaryKey: networkProfileKey) as? String
    let sourceCommit = value(sourceCommitKey, values: values, bundle: bundle)
    let sourceTree = value(sourceTreeKey, values: values, bundle: bundle)
    let appBuildSHA256 = value(appBuildSHA256Key, values: values, bundle: bundle)
    let simulatorID = value(simulatorIDKey, values: values, bundle: bundle)
    if runIDValue?.isEmpty != false, blossomValue?.isEmpty != false {
      throw XCTSkip("remote qualification inputs were not selected")
    }
    guard let runID = runIDValue,
      runID.range(of: "^[a-z0-9][a-z0-9-]{6,62}[a-z0-9]$", options: .regularExpression) != nil,
      let blossom = blossomValue,
      !blossom.isEmpty,
      let networkProfile,
      networkProfile == "public" || networkProfile == "simulator"
    else {
      throw QualificationError.missingEnvironment
    }
    _ = try QualificationEndpointPolicy(
      relayURLs: separated(relayValue),
      blossomOrigins: separated(blossom),
      profile: networkProfile
    )
    if networkProfile == "simulator" {
      guard
        let fixtureControl,
        let fixtureEvidence,
        fixtureControl.utf8.count <= 1024,
        fixtureEvidence.utf8.count <= 1024,
        fixtureControl.hasPrefix("/"),
        fixtureEvidence.hasPrefix("/")
      else {
        throw QualificationError.missingEnvironment
      }
    }
    let evidenceBinding = PersonaEvidenceBinding(
      sourceCommit: sourceCommit,
      sourceTree: sourceTree,
      appBuildSHA256: appBuildSHA256,
      simulatorID: simulatorID
    )
    return Self(
      runID: runID,
      relayURLs: separated(relayValue),
      blossomOrigins: separated(blossom),
      fixtureControl: fixtureControl.flatMap { $0.isEmpty ? nil : $0 },
      fixtureEvidence: fixtureEvidence.flatMap { $0.isEmpty ? nil : $0 },
      networkProfile: networkProfile,
      evidenceBinding: evidenceBinding
    )
  }

  private static func value(
    _ key: String,
    values: [String: String],
    bundle: Bundle
  ) -> String? {
    (values[key] ?? bundle.object(forInfoDictionaryKey: key) as? String)
      .flatMap { $0.isEmpty ? nil : $0 }
  }

  private static func separated(_ value: String?) -> [String] {
    (value ?? "").split(separator: ",").map(String.init).filter { !$0.isEmpty }
  }
}

private struct PersonaEvidenceBinding {
  let sourceCommit: String
  let sourceTree: String
  let appBuildSHA256: String
  let simulatorID: String

  init?(
    sourceCommit: String?,
    sourceTree: String?,
    appBuildSHA256: String?,
    simulatorID: String?
  ) {
    guard let sourceCommit,
      sourceCommit.range(of: "^[0-9a-f]{40}$", options: .regularExpression) != nil,
      let sourceTree,
      sourceTree.range(of: "^[0-9a-f]{40}$", options: .regularExpression) != nil,
      let appBuildSHA256,
      appBuildSHA256.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil,
      let simulatorID,
      simulatorID.uppercased().range(
        of: "^[A-F0-9-]{36}$",
        options: .regularExpression
      ) != nil
    else { return nil }
    self.sourceCommit = sourceCommit
    self.sourceTree = sourceTree
    self.appBuildSHA256 = appBuildSHA256
    self.simulatorID = simulatorID.uppercased()
  }
}

private struct PersonaAttemptTestInvocation: Encodable {
  let target: String
  let identifier: String
  let action: String
  let configuration: String
}

private struct PersonaAttemptSource: Encodable {
  let commit: String
  let tree: String
}

private struct PersonaAttemptSimulator: Encodable {
  let udid: String
  let os: String
  let architecture: String
}

private struct PersonaAttemptUIObservation: Encodable {
  let validationAttempted: Bool
  let validationRejected: Bool
  let retryAttempts: Int
  let relaunches: Int
  let retentionVerified: Bool
  let todayProjectionVerified: Bool

  enum CodingKeys: String, CodingKey {
    case validationAttempted = "validation_attempted"
    case validationRejected = "validation_rejected"
    case retryAttempts = "retry_attempts"
    case relaunches
    case retentionVerified = "retention_verified"
    case todayProjectionVerified = "today_projection_verified"
  }
}

private struct PersonaInteractionObservation {
  let validationAttempted: Bool
  let validationRejected: Bool
  let retryAttempts: Int
  let relaunches: Int
  let progressiveDisclosure: Bool
  let accessibilitySemantics: Bool
  let keyboardFocus: Bool
  let visibleActions: Bool
}

private struct PersonaAttemptObservation {
  let interaction: PersonaInteractionObservation
  let network: PersonaAttemptNetworkObservation
  let todayProjectionVerified: Bool
  let retentionVerified: Bool

  var ui: PersonaAttemptUIObservation {
    PersonaAttemptUIObservation(
      validationAttempted: interaction.validationAttempted,
      validationRejected: interaction.validationRejected,
      retryAttempts: interaction.retryAttempts,
      relaunches: interaction.relaunches,
      retentionVerified: retentionVerified,
      todayProjectionVerified: todayProjectionVerified
    )
  }

  func confirmingRetention() -> Self {
    Self(
      interaction: interaction,
      network: network,
      todayProjectionVerified: todayProjectionVerified,
      retentionVerified: true
    )
  }
}

private struct PersonaAttemptNetworkObservation: Encodable {
  let state: String
  let acceptedConnections: Int
  let rejectedConnections: Int
  let nonLoopbackAttempts: Int
  let subscriptions: Int
  let acceptedEvents: Int
  let acceptedUploads: Int
  let retrievals: Int
  let unintendedPublications: Int
  let eventsAcceptedDuringExpectedFailure: Int
  let finalCandidateDataLoss: Int

  enum CodingKeys: String, CodingKey {
    case state
    case acceptedConnections = "accepted_connections"
    case rejectedConnections = "rejected_connections"
    case nonLoopbackAttempts = "non_loopback_attempts"
    case subscriptions
    case acceptedEvents = "accepted_events"
    case acceptedUploads = "accepted_uploads"
    case retrievals
    case unintendedPublications = "unintended_publications"
    case eventsAcceptedDuringExpectedFailure = "events_accepted_during_expected_failure"
    case finalCandidateDataLoss = "final_candidate_data_loss"
  }
}

private struct PersonaFixtureNetworkSnapshot {
  private static let keys: Set<String> = [
    "schema", "schema_version", "personas", "flow_counts", "accepted_events",
    "event_kind_counts", "upload_attempts", "accepted_uploads", "retrievals",
    "distinct_identities", "unknown_attempts", "duplicate_attempts",
    "expected_failure_rejections", "events_accepted_during_expected_failures",
    "accepted_connections", "rejected_connections", "non_loopback_attempts",
    "production_network_contacts", "unintended_publications", "final_candidate_data_loss",
  ]

  let acceptedConnections: Int
  let rejectedConnections: Int
  let nonLoopbackAttempts: Int
  let productionNetworkContacts: Int
  let subscriptions: Int
  let acceptedEvents: Int
  let acceptedUploads: Int
  let retrievals: Int
  let unintendedPublications: Int
  let eventsAcceptedDuringExpectedFailures: Int
  let finalCandidateDataLoss: Int

  init(_ value: [String: Any]) throws {
    guard Set(value.keys) == Self.keys,
      value["schema"] as? String == "radroots.ios.local-social.persona-evidence.v1",
      value["schema_version"] as? Int == 1,
      let acceptedConnections = value["accepted_connections"] as? Int,
      let rejectedConnections = value["rejected_connections"] as? Int,
      let nonLoopbackAttempts = value["non_loopback_attempts"] as? Int,
      let productionNetworkContacts = value["production_network_contacts"] as? Int,
      let acceptedEvents = value["accepted_events"] as? Int,
      let acceptedUploads = value["accepted_uploads"] as? Int,
      let retrievals = value["retrievals"] as? Int,
      let unintendedPublications = value["unintended_publications"] as? Int,
      let eventsAccepted = value["events_accepted_during_expected_failures"] as? Int,
      let finalCandidateDataLoss = value["final_candidate_data_loss"] as? Int,
      let personas = value["personas"] as? [[String: Any]],
      personas.count == 5,
      [
        acceptedConnections, rejectedConnections, nonLoopbackAttempts,
        productionNetworkContacts,
        acceptedEvents, acceptedUploads, retrievals, unintendedPublications,
        eventsAccepted, finalCandidateDataLoss,
      ].allSatisfy({ 0 ... 61440 ~= $0 })
    else {
      throw QualificationError.invalidFixtureEvidence
    }
    let subscriptions = try personas.reduce(into: 0) { total, persona in
      guard let value = persona["subscriptions"] as? Int, 0 ... 4096 ~= value else {
        throw QualificationError.invalidFixtureEvidence
      }
      total += value
    }
    self.acceptedConnections = acceptedConnections
    self.rejectedConnections = rejectedConnections
    self.nonLoopbackAttempts = nonLoopbackAttempts
    self.productionNetworkContacts = productionNetworkContacts
    self.subscriptions = subscriptions
    self.acceptedEvents = acceptedEvents
    self.acceptedUploads = acceptedUploads
    self.retrievals = retrievals
    self.unintendedPublications = unintendedPublications
    eventsAcceptedDuringExpectedFailures = eventsAccepted
    self.finalCandidateDataLoss = finalCandidateDataLoss
  }

  func delta(from prior: Self) throws -> PersonaAttemptNetworkObservation {
    let current = [
      acceptedConnections, rejectedConnections, nonLoopbackAttempts, subscriptions,
      productionNetworkContacts,
      acceptedEvents, acceptedUploads, retrievals, unintendedPublications,
      eventsAcceptedDuringExpectedFailures, finalCandidateDataLoss,
    ]
    let previous = [
      prior.acceptedConnections, prior.rejectedConnections, prior.nonLoopbackAttempts,
      prior.subscriptions, prior.productionNetworkContacts, prior.acceptedEvents,
      prior.acceptedUploads, prior.retrievals,
      prior.unintendedPublications, prior.eventsAcceptedDuringExpectedFailures,
      prior.finalCandidateDataLoss,
    ]
    guard zip(current, previous).allSatisfy({ pair in pair.0 >= pair.1 }) else {
      throw QualificationError.invalidFixtureEvidence
    }
    let values = zip(current, previous).map { pair in pair.0 - pair.1 }
    guard values[2] == 0, values[4] == 0 else {
      throw QualificationError.invalidEndpointPolicy
    }
    return PersonaAttemptNetworkObservation(
      state: "measured",
      acceptedConnections: values[0],
      rejectedConnections: values[1],
      nonLoopbackAttempts: values[2],
      subscriptions: values[3],
      acceptedEvents: values[5],
      acceptedUploads: values[6],
      retrievals: values[7],
      unintendedPublications: values[8],
      eventsAcceptedDuringExpectedFailure: values[9],
      finalCandidateDataLoss: values[10]
    )
  }
}

private struct PersonaAttemptAccessibility: Encodable {
  let locale: String
  let contentSize: String
  let reduceMotion: Bool
  let progressiveDisclosure: Bool
  let labelsValuesTraits: Bool
  let keyboardFocus: Bool
  let visibleActions: Bool
  let voiceoverUserObserved: Bool

  enum CodingKeys: String, CodingKey {
    case locale
    case contentSize = "content_size"
    case reduceMotion = "reduce_motion"
    case progressiveDisclosure = "progressive_disclosure"
    case labelsValuesTraits = "labels_values_traits"
    case keyboardFocus = "keyboard_focus"
    case visibleActions = "visible_actions"
    case voiceoverUserObserved = "voiceover_user_observed"
  }
}

private struct PersonaAttemptArtifactDigest: Encodable {
  let role: String
  let sha256: String
}

private struct PersonaAttemptEvidence: Encodable {
  let schema: String
  let schemaVersion: UInt16
  let testInvocation: PersonaAttemptTestInvocation
  let source: PersonaAttemptSource
  let appBuildSHA256: String
  let simulator: PersonaAttemptSimulator
  let runID: String
  let personaRunID: String
  let personaAlias: String
  let attemptID: String
  let attemptOrder: Int
  let flow: String
  let expectedFailure: String
  let publicIdentitySHA256: String
  let endpointPolicySHA256: String
  let uiObservation: PersonaAttemptUIObservation
  let networkObservation: PersonaAttemptNetworkObservation
  let accessibility: PersonaAttemptAccessibility
  let artifactDigests: [PersonaAttemptArtifactDigest]

  enum CodingKeys: String, CodingKey {
    case schema
    case schemaVersion = "schema_version"
    case testInvocation = "test_invocation"
    case source
    case appBuildSHA256 = "app_build_sha256"
    case simulator
    case runID = "run_id"
    case personaRunID = "persona_run_id"
    case personaAlias = "persona_alias"
    case attemptID = "attempt_id"
    case attemptOrder = "attempt_order"
    case flow
    case expectedFailure = "expected_failure"
    case publicIdentitySHA256 = "public_identity_sha256"
    case endpointPolicySHA256 = "endpoint_policy_sha256"
    case uiObservation = "ui_observation"
    case networkObservation = "network_observation"
    case accessibility
    case artifactDigests = "artifact_digests"
  }
}

private struct BootstrapReceipt: Codable {
  let schema: String
  let schemaVersion: UInt16
  let runID: String
  let publicKey: String
  let relayURLs: [String]
  let blossomOrigins: [String]
  let privateKeyPresent: Bool
  let interactiveAuthenticationRequired: Bool
}

private struct PersonaSuite: Decodable {
  let schema: String
  let schemaVersion: UInt16
  let locale: String
  let mediaFixtureSHA256: String
  let personas: [Persona]

  enum CodingKeys: String, CodingKey {
    case schema
    case schemaVersion = "schema_version"
    case locale
    case mediaFixtureSHA256 = "media_fixture_sha256"
    case personas
  }
}

private struct Persona: Decodable {
  let alias: String
  let syntheticAgeBand: String
  let interactionProfile: String
  let attempts: [PersonaAttempt]

  enum CodingKeys: String, CodingKey {
    case alias
    case syntheticAgeBand = "synthetic_age_band"
    case interactionProfile = "interaction_profile"
    case attempts
  }
}

private struct PersonaAttempt: Decodable {
  let id: String
  let order: Int
  let flow: PersonaFlow
  let marker: String
  let expectedFailure: String

  enum CodingKeys: String, CodingKey {
    case id
    case order
    case flow
    case marker
    case expectedFailure = "expected_failure"
  }
}

private enum PersonaFlow: String, Decodable {
  case update = "Update"
  case photoUpdate = "PhotoUpdate"
  case ask = "Ask"
  case event = "Event"
  case foodAvailability = "FoodAvailability"

  var uiLabel: String {
    switch self {
    case .update: "Update"
    case .photoUpdate: "Photo update"
    case .ask: "Ask"
    case .event: "Event"
    case .foodAvailability: "Food availability"
    }
  }
}

private enum QualificationError: Error {
  case missingEnvironment
  case invalidEndpointPolicy
  case invalidFixtureEvidence
  case invalidPublicKey
  case missingProductSurface
  case productSubmissionFailed
  case invalidPersonaFixture
}
