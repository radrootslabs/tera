import SwiftUI
import UIKit

struct TeraMediaPermissionGuidance: View {
  let support: TeraAddMediaSupport
  let recheck: @MainActor () async -> Void
  @Environment(\.openURL) private var openURL

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text(support.cameraAccess.guidance)
      Text(support.library
        ? "You can select photos from Library without full photo-library access, or keep editing and save without adding media."
        : "Photo selection is unavailable. You can keep editing and save without adding media.")
      if support.cameraAccess == .denied {
        Button("Open camera settings") {
          if let url = URL(string: UIApplication.openSettingsURLString) {
            openURL(url)
          }
        }
        .accessibilityIdentifier("tera.add.camera.settings")
      }
      Button("Check camera access again") { Task { await recheck() } }
        .accessibilityIdentifier("tera.add.camera.recheck")
    }
    .accessibilityElement(children: .contain)
  }
}
