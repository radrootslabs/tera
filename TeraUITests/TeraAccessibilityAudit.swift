import XCTest

@MainActor
final class TeraAccessibilityAudit {
  private let test: XCTestCase
  private var findings: [String] = []

  init(test: XCTestCase) {
    self.test = test
  }

  static func assertNoFindings(_ findings: [String], file: StaticString = #filePath, line: UInt = #line) {
    XCTAssertTrue(findings.isEmpty, "Unresolved accessibility findings:\n" + findings.joined(separator: "\n"),
                  file: file, line: line)
  }

  static func retainFinding(_ description: String, elementLabel: String?, in findings: inout [String]) -> Bool {
    findings.append("\(description): \(elementLabel ?? "unidentified")")
    // Continuing a native scan does not waive the mandatory caller assertion.
    return true
  }

  static func contrastDisposition(label: String?, isEnabled: Bool, cameraIsDisabled: Bool,
                                  isRequestedTarget: Bool,
                                  isPartlyOccluded: Bool) -> TeraAccessibilityContrastDisposition
  {
    guard let label else { return .retain }
    if label == "Camera", !isEnabled, cameraIsDisabled {
      return .disabledCamera
    }
    return !isRequestedTarget && isPartlyOccluded ? .recheck : .retain
  }

  static func failContrastRechecks(file: StaticString = #filePath, line: UInt = #line) {
    XCTFail("Not all partially obscured contrast findings passed an unobscured recheck", file: file, line: line)
  }

  func run(_ app: XCUIApplication, dynamicType: Bool = false, includeContrast: Bool = true) throws -> [String] {
    // Native scans can move focus and scroll position. Audit clipping before
    // those scans and retain every finding, including unidentified ones.
    var types: XCUIAccessibilityAuditType = [.elementDetection, .hitRegion,
                                             .sufficientElementDescription, .trait]
    if dynamicType {
      types.insert(.dynamicType)
    }
    let before = XCTAttachment(string: app.debugDescription)
    before.name = "Accessibility tree before audit"
    before.lifetime = .keepAlways
    test.add(before)
    let screenshot = XCTAttachment(screenshot: app.screenshot())
    screenshot.name = "Screen before audit"
    screenshot.lifetime = .keepAlways
    test.add(screenshot)
    try scan(app, types: .textClipped)
    if includeContrast {
      try auditContrast(app)
    }
    try scan(app, types: types)
    return findings
  }

  private func scan(_ app: XCUIApplication, types: XCUIAccessibilityAuditType) throws {
    try app.performAccessibilityAudit(for: types) { issue in
      let detail = XCTAttachment(string: "Audit type: \(issue.auditType.rawValue)\nAudit: \(issue.detailedDescription)\nElement: \(issue.element?.debugDescription ?? "unidentified")")
      detail.lifetime = .keepAlways
      self.test.add(detail)
      let tree = XCTAttachment(string: app.debugDescription)
      tree.name = "Audit finding accessibility tree"
      tree.lifetime = .keepAlways
      self.test.add(tree)
      let screen = XCTAttachment(screenshot: app.screenshot())
      screen.name = "Audit finding screen"
      screen.lifetime = .keepAlways
      self.test.add(screen)
      if let element = issue.element {
        let fresh = TeraAuditTarget(element, app: app).resolve(in: app)
        let geometry = XCTAttachment(string: "Current target: \(fresh.debugDescription)")
        geometry.lifetime = .keepAlways
        self.test.add(geometry)
      }
      return self.retainFinding(issue)
    }
  }
}

@MainActor
enum TeraAccessibilityNavigation {
  static func qualifySupportSettings(_ app: XCUIApplication, test: XCTestCase) throws -> [String] {
    var findings: [String] = []
    for identifier in ["tera.settings.diagnostics.contents", "tera.settings.diagnostics.exclusions",
                       "tera.settings.support.contact", "tera.settings.support.review",
                       "tera.settings.support.local_removal", "tera.settings.support.remote_copies"]
    {
      let target = app.descendants(matching: .any)
        .matching(NSPredicate(format: "identifier == %@", identifier)).firstMatch
      scroll(app, to: target, requiresHit: identifier == "tera.settings.support.contact", test: test)
      XCTAssertTrue(target.exists)
      if identifier == "tera.settings.support.contact" {
        XCTAssertTrue(target.isHittable)
        XCTAssertGreaterThanOrEqual(target.frame.height, 44)
      }
      findings += try TeraAccessibilityAudit(test: test).run(app)
    }
    return findings
  }

  static func viewport(_ app: XCUIApplication) -> (top: CGFloat, bottom: CGFloat) {
    let navigation = app.navigationBars.firstMatch
    let tabs = app.tabBars.firstMatch
    // A presented account/import sheet covers the underlying tab bar. Its
    // foreground content must use the sheet viewport, not the covered tabs.
    let top = navigation.exists ? navigation.frame.maxY : 64
    let bottom = tabs.exists && tabs.isHittable ? tabs.frame.minY : app.frame.maxY - 40
    return (top, bottom)
  }

  static func scroll(_ app: XCUIApplication, to element: XCUIElement, up: Bool = true,
                     requiresHit: Bool = true, test: XCTestCase)
  {
    var forward = up
    var previousViewport: [String] = []
    var stationaryDrags = 0
    for _ in 0 ..< 28 {
      let position = scrollPosition(app)
      let frame = element.exists ? String(describing: element.frame) : "not instantiated"
      XCTContext.runActivity(named: "Accessibility scroll: position \(position ?? "unknown"), target \(frame)") { _ in }
      if isVisible(element, app: app, requiresHit: requiresHit) {
        return
      }
      // Lazy forms revise their estimated content height while revealing rows.
      // A repeated percentage is not an edge. Reverse only after actual
      // visible content has stopped moving across two completed drags.
      let viewport = visibleContent(app)
      stationaryDrags = !viewport.isEmpty && viewport == previousViewport ? stationaryDrags + 1 : 0
      if stationaryDrags == 2 {
        forward.toggle()
        stationaryDrags = 0
      }
      previousViewport = viewport
      if element.exists, element.frame.width > 0, element.frame.height > 0 {
        let (top, bottom) = TeraAccessibilityNavigation.viewport(app)
        let frame = element.frame
        let distance = bottom - top - 40
        let delta = max(-distance, min(distance, frame.midY - (top + bottom) / 2))
        if abs(delta) > 8 {
          drag(app, amount: -delta, top: top, bottom: bottom)
          continue
        }
      }
      // Audit scans can leave a lazy form on either side of the target.
      // Reverse only when scrolling has stopped making progress, so a long
      // maximum-text form can use the whole bound to reach the opposite end.
      let (top, bottom) = TeraAccessibilityNavigation.viewport(app)
      let delta = (bottom - top - 40) * (forward ? -0.7 : 0.7)
      drag(app, amount: delta, top: top, bottom: bottom)
    }
    // The final drag can expose a lazy element. Check its settled frame before
    // declaring the bounded search unsuccessful.
    if isVisible(element, app: app, requiresHit: requiresHit) {
      return
    }
    let screenshot = XCTAttachment(screenshot: app.screenshot())
    screenshot.lifetime = .keepAlways
    test.add(screenshot)
    let (top, bottom) = TeraAccessibilityNavigation.viewport(app)
    let exists = element.exists
    let frame = exists ? element.frame : .zero
    let detail = exists ? element.debugDescription : "Target is not instantiated"
    let geometry = String(format: "minY=%.17g maxY=%.17g top=%.17g bottom=%.17g", frame.minY, frame.maxY, top, bottom)
    XCTFail("Required accessibility element is unreachable: exists=\(exists) hittable=\(exists && element.isHittable) \(geometry). \(detail).\n\(app.debugDescription)")
  }

  private static func isVisible(_ element: XCUIElement, app: XCUIApplication, requiresHit: Bool) -> Bool {
    guard element.exists, !requiresHit || element.isHittable else { return false }
    let (top, bottom) = TeraAccessibilityNavigation.viewport(app)
    let frame = element.frame
    return contains(frame, top: top, bottom: bottom)
  }

  static func contains(_ frame: CGRect, top: CGFloat, bottom: CGFloat) -> Bool {
    // UIKit's scaled geometry can differ by a few floating-point units at the
    // same boundary (observed 189 versus 189.00000000000011). Eight ulps are
    // far below a screen pixel; actual clipping still fails the native audits.
    frame.width > 0 && frame.height > 0
      && frame.minY >= top - top.ulp * 8
      && frame.maxY <= bottom + bottom.ulp * 8
  }

  private static func scrollPosition(_ app: XCUIApplication) -> String? {
    // Text editors have their own scrollbars. Progress belongs to the outer
    // form, whose visible scrollbar is farther right than those nested editors.
    let bars = app.otherElements.matching(NSPredicate(format: "label BEGINSWITH %@", "Vertical scroll bar"))
      .allElementsBoundByIndex.filter { $0.frame.height > 0 && $0.frame.intersects(app.frame) }
    let outer = bars.max { lhs, rhs in
      lhs.frame.maxX == rhs.frame.maxX ? lhs.frame.height < rhs.frame.height : lhs.frame.maxX < rhs.frame.maxX
    }
    return outer?.value as? String
  }

  private static func visibleContent(_ app: XCUIApplication) -> [String] {
    let (top, bottom) = TeraAccessibilityNavigation.viewport(app)
    return app.staticTexts.allElementsBoundByIndex.compactMap { text in
      let frame = text.frame
      guard frame.height > 0, frame.maxY > top, frame.minY < bottom else { return nil }
      return "\(text.identifier):\(text.label):\(frame.minY.rounded()):\(frame.maxY.rounded())"
    }
  }

  private static func drag(_ app: XCUIApplication, amount: CGFloat, top: CGFloat, bottom: CGFloat) {
    // Application coordinates can resolve to a zero-width origin. The actual
    // window gives a stable point in the form margin, outside nested editors.
    let window = app.windows.firstMatch
    let origin = window.coordinate(withNormalizedOffset: .zero)
    // Start and end in the middle half of the scrollable area. The system
    // bars can intercept gestures near the nominal safe-area boundaries.
    let span = bottom - top
    let delta = max(-span / 2, min(span / 2, amount))
    let startY = (delta < 0 ? bottom - span / 4 : top + span / 4) - window.frame.minY
    let start = origin.withOffset(CGVector(dx: 20, dy: startY))
    // A second withOffset replaces the previous offset. Calculate both ends
    // from the same origin so the destination stays inside the viewport.
    let finish = origin.withOffset(CGVector(dx: 20, dy: startY + delta))
    // Hold at the destination to stop momentum before reading the next frame.
    start.press(forDuration: 0.05, thenDragTo: finish,
                withVelocity: .slow, thenHoldForDuration: 0.3)
  }
}

/// Contrast sampling under translucent navigation/tab bars can report a
/// partially occluded element. Every such finding must pass a new contrast
/// audit with its entire frame exposed. Unidentified findings remain fatal.
private extension TeraAccessibilityAudit {
  func auditContrast(_ app: XCUIApplication) throws {
    var pending: [String: TeraAuditTarget] = [:]
    var verified = Set<String>()
    var requested: TeraAuditTarget?
    for _ in 0 ..< 20 {
      let target = requested?.resolve(in: app)
      let targetKey = requested?.key
      if let target {
        // A menu's static label has no separate tap action. It still must be
        // fully exposed and pass the actual native contrast re-audit below.
        TeraAccessibilityNavigation.scroll(app, to: target, up: requested?.scrollUp ?? true,
                                           requiresHit: requested?.type != .staticText, test: test)
      }
      let findingsBefore = findings.count
      try app.performAccessibilityAudit(for: .contrast) { issue in
        let observation = XCTAttachment(string: "Contrast observation: \(issue.detailedDescription)\n\(issue.element?.debugDescription ?? "unidentified")")
        observation.lifetime = .keepAlways
        self.test.add(observation)
        guard let element = issue.element else { return self.retainFinding(issue) }
        // SC 1.4.3 exempts inactive controls. Keep this bound to the actual
        // disabled camera control; no active text or clipping finding is exempt.
        let key = self.contrastKey(element)
        let camera = app.buttons["Camera"]
        let disposition = Self.contrastDisposition(label: element.label, isEnabled: element.isEnabled,
                                                   cameraIsDisabled: camera.exists && !camera.isEnabled,
                                                   isRequestedTarget: key == targetKey,
                                                   isPartlyOccluded: self.isPartlyOccluded(element, app: app))
        switch disposition {
        case .disabledCamera: return true
        case .retain: return self.retainFinding(issue)
        case .recheck: break
        }
        if !verified.contains(key) {
          pending[key] = TeraAuditTarget(element, app: app)
        }
        let evidence = XCTAttachment(string: "Contrast requires unobscured recheck: \(element.debugDescription)")
        evidence.lifetime = .keepAlways
        self.test.add(evidence)
        return true
      }
      if let target, let targetKey {
        let (top, bottom) = TeraAccessibilityNavigation.viewport(app)
        XCTAssertTrue((requested?.type == .staticText || target.isHittable)
                        && TeraAccessibilityNavigation.contains(target.frame, top: top, bottom: bottom),
                      "Contrast recheck moved outside the viewport")
        if findings.count == findingsBefore {
          verified.insert(targetKey)
        }
      }
      guard let next = pending.keys.sorted().first else { return }
      requested = pending.removeValue(forKey: next)
    }
    Self.failContrastRechecks()
  }

  func retainFinding(_ issue: XCUIAccessibilityAuditIssue) -> Bool {
    Self.retainFinding(issue.compactDescription, elementLabel: issue.element?.label, in: &findings)
  }

  func contrastKey(_ element: XCUIElement) -> String {
    "\(element.elementType.rawValue):\(element.identifier):\(element.label)"
  }

  func isPartlyOccluded(_ element: XCUIElement, app: XCUIApplication) -> Bool {
    let frame = element.frame
    let (top, bottom) = TeraAccessibilityNavigation.viewport(app)
    guard frame.height > 0, frame.height < bottom - top - 16,
          frame.minX >= app.frame.minX, frame.maxX <= app.frame.maxX else { return false }
    return frame.minY < app.frame.maxY && frame.maxY > app.frame.minY
      && !TeraAccessibilityNavigation.contains(frame, top: top, bottom: bottom)
  }
}

enum TeraAccessibilityContrastDisposition: Equatable {
  case retain, recheck, disabledCamera
}

private struct TeraAuditTarget {
  let type: XCUIElement.ElementType
  let identifier: String
  let label: String
  let scrollUp: Bool

  var key: String {
    "\(type.rawValue):\(identifier):\(label)"
  }

  @MainActor init(_ element: XCUIElement, app: XCUIApplication) {
    type = element.elementType
    identifier = element.identifier
    label = element.label
    scrollUp = element.frame.midY > app.frame.midY
  }

  @MainActor func resolve(in app: XCUIApplication) -> XCUIElement {
    let predicate = identifier.isEmpty ? NSPredicate(format: "label == %@", label)
      : NSPredicate(format: "identifier == %@ AND label == %@", identifier, label)
    // UIKit-backed SwiftUI section headings expose different legacy and modern
    // automation types. Match the stable identity and label across both trees.
    return app.descendants(matching: .any).matching(predicate).firstMatch
  }
}

@MainActor
enum TeraAccessibilitySettings {
  static func setSystemReduceMotion(_ enabled: Bool, restoring test: XCTestCase? = nil) -> Bool {
    let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
    // This helper operates Settings, not the app under qualification. Keep
    // its navigation stable while Tera uses the real maximum system size.
    settings.launchArguments = ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryL"]
    settings.launch()
    if settings.navigationBars["Settings"].exists {
      for _ in 0 ..< 3 {
        settings.swipeDown()
      }
    }
    for _ in 0 ..< 8 {
      if settings.switches["Reduce Motion"].exists {
        break
      }
      let motion = settings.buttons["Motion"].exists ? settings.buttons["Motion"] : settings.cells["Motion"]
      let accessibility = settings.buttons["Accessibility"].exists ? settings.buttons["Accessibility"] : settings.cells["Accessibility"]
      if motion.exists, motion.isHittable {
        motion.tap()
      } else if accessibility.exists, accessibility.isHittable {
        accessibility.tap()
      } else if settings.navigationBars.buttons.firstMatch.exists {
        settings.navigationBars.buttons.firstMatch.tap()
      } else {
        settings.swipeUp()
      }
    }
    let toggle = settings.switches["Reduce Motion"]
    XCTAssertTrue(toggle.waitForExistence(timeout: 5), settings.debugDescription)
    let original = toggle.value as? String == "1"
    // Register restoration before changing the setting, including on failure.
    if let test {
      let originalSetting = XCTAttachment(string: "Original system Reduce Motion: \(original)")
      originalSetting.lifetime = .keepAlways
      test.add(originalSetting)
      test.addTeardownBlock {
        await MainActor.run {
          _ = setSystemReduceMotion(original)
        }
      }
    }
    if original != enabled {
      // Settings exposes the entire row as the switch accessibility element;
      // its label does not toggle the visual control on this OS version.
      toggle.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap()
    }
    let settled = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", enabled ? "1" : "0"), object: toggle)
    XCTAssertEqual(XCTWaiter.wait(for: [settled], timeout: 5), .completed, settings.debugDescription)
    settings.terminate()
    return original
  }
}
