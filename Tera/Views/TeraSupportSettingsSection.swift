import Foundation
import SwiftUI

/// Read the approved packaged contact. No message or diagnostics are sent
/// until the person chooses an external action and submits it themselves.
enum TeraSupportContact {
  static func mailURL(email: String?) -> URL? {
    guard email == "support@radroots.org" else { return nil }
    return URL(string: "mailto:support@radroots.org")
  }

  static var packagedMailURL: URL? {
    mailURL(email: Bundle.main.object(forInfoDictionaryKey: "TeraSupportEmail") as? String)
  }
}

struct TeraSupportSettingsSection: View {
  var body: some View {
    Section("Support and privacy") {
      if let contact = TeraSupportContact.packagedMailURL {
        Link("Contact Radroots Support", destination: contact)
          .accessibilityIdentifier("tera.settings.support.contact")
      }
      Text("Review a diagnostics export before you share it.")
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityIdentifier("tera.settings.support.review")
      Text("Removing a local signing key does not erase remote copies.")
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityIdentifier("tera.settings.support.local_removal")
      Text("Relays and other people may keep copies of public posts.")
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityIdentifier("tera.settings.support.remote_copies")
    }
  }
}
