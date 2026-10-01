import SwiftUI

struct TeraPublicationDisclosure: View {
  var body: some View {
    Section("Public publication") {
      Text("Submit publishes to public Nostr relays. A local network helps you discover nearby posts; it is not a private group.")
        .accessibilityIdentifier("tera.add.disclosure.public")
      Text("Photos are uploaded to a separate public photo service. Anyone who obtains their links may view or copy them. Prepared photos have embedded location and camera metadata removed, but visible details can still identify people and places.")
        .accessibilityIdentifier("tera.add.disclosure.media")
      Text("Location text you enter is public when you submit. Include an exact address only if you intend to share it publicly. Tera does not request your device location; you can enter a less precise description or leave optional location blank.")
        .accessibilityIdentifier("tera.add.disclosure.location")
      Text("Save draft keeps editing on this device. Submission and retraction cannot guarantee that remote copies will be removed.")
    }
  }
}
