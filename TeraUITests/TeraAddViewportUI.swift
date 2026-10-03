import XCTest

extension TeraRemoteQualificationUITests {
  @MainActor
  func isVisibleInAddViewport(
    _ element: XCUIElement,
    root: XCUIElement,
    above obstruction: XCUIElement,
    below navigation: XCUIElement
  ) -> Bool {
    guard element.exists, element.isHittable, root.exists, obstruction.exists, navigation.exists else {
      return false
    }
    let frame = element.frame
    let rootFrame = root.frame
    return frame.height > 0
      && frame.minY >= max(rootFrame.minY, navigation.frame.maxY)
      && frame.maxY <= min(rootFrame.maxY, obstruction.frame.minY)
  }
}
