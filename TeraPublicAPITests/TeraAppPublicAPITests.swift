import SwiftUI
import TeraApp
import UIKit
import XCTest

final class TeraAppPublicAPITests: XCTestCase {
    @MainActor
    func testSupportedPublicSurfaceCompilesForAnExternalConsumer() {
        XCTAssertEqual(TeraAppRelease.version, "0.1.0-alpha")
        let appView: any View = TeraAppView()
        let appDelegate: any UIApplicationDelegate = TeraAppDelegate()
        _ = appView
        _ = appDelegate
    }
}
