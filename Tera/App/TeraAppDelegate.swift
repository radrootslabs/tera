import UIKit

public final class TeraAppDelegate: NSObject, UIApplicationDelegate {
    override public init() {
        super.init()
    }

    public func application(
      _ application: UIApplication,
      didFinishLaunchingWithOptions _: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        #if DEBUG
            if (try? TeraRemoteQualificationEnvironment.current()) != nil {
                application.isIdleTimerDisabled = true
            }
        #endif
        return true
    }

    public func application(
      _: UIApplication,
      handleEventsForBackgroundURLSession identifier: String,
      completionHandler: @escaping () -> Void
    ) {
        let completion = TeraCompletionOnce(completionHandler)
        Task {
            await TeraBackgroundEventRouter.shared.handle(
              identifier: identifier,
              completion: completion
            )
        }
    }
}
