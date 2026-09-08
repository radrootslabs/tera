import SwiftUI
import TeraApp

@main
struct TeraApp: App {
    @UIApplicationDelegateAdaptor(TeraAppDelegate.self) private var appDelegate

    var body: some Scene {
        WindowGroup {
            TeraAppView()
        }
    }
}
