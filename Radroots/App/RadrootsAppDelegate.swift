import UIKit

public final class RadrootsAppDelegate: NSObject, UIApplicationDelegate {
    override public init() {
        super.init()
    }

    public func application(
      _ application: UIApplication,
      didFinishLaunchingWithOptions _: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        #if DEBUG
            if (try? RadrootsRemoteQualificationEnvironment.current()) != nil {
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
        let completion = RadrootsCompletionOnce(completionHandler)
        Task {
            await RadrootsBackgroundEventRouter.shared.handle(
              identifier: identifier,
              completion: completion
            )
        }
    }
}
