import Combine
import SwiftUI
import UIKit

struct RadrootsProvider<Content: View>: View {
    @Environment(\.scenePhase) private var scenePhase
    @StateObject private var appModel: RadrootsAppModel
    private let content: () -> Content

    init(
      appModel: @autoclosure @escaping () -> RadrootsAppModel = RadrootsAppModel(),
      @ViewBuilder content: @escaping () -> Content
    ) {
        _appModel = StateObject(wrappedValue: appModel())
        self.content = content
    }

    var body: some View {
        content()
            .environmentObject(appModel)
            .environmentObject(appModel.diagnosticsStore)
            .task { await appModel.resume() }
            .onChange(of: scenePhase) { _, phase in
                if phase == .active {
                    Task { await appModel.resume() }
                } else if phase == .background {
                    Task { await appModel.suspend() }
                }
            }
            .onReceive(
                NotificationCenter.default.publisher(
                    for: UIApplication.protectedDataDidBecomeAvailableNotification
                )
            ) { _ in
                Task { await appModel.updateProtectedDataAvailability(true) }
            }
            .onReceive(
                NotificationCenter.default.publisher(
                    for: UIApplication.protectedDataWillBecomeUnavailableNotification
                )
            ) { _ in
                Task { await appModel.updateProtectedDataAvailability(false) }
            }
    }
}
