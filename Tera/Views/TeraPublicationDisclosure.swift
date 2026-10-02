import SwiftUI

struct TeraPublicationDisclosure: View {
  var body: some View {
    Section("Public publication") {
      Text("Submit publishes to public Nostr relays.")
        .accessibilityIdentifier("tera.add.disclosure.public")
      Text("A local network helps you discover nearby posts; it is not a private group.")
      Text("Photos are uploaded to a separate public photo service.")
        .accessibilityIdentifier("tera.add.disclosure.media")
      Text("Anyone who obtains their links may view or copy them.")
      Text("Prepared photos have embedded location and camera metadata removed.")
      Text("Visible details can still identify people and places.")
      Text("Location text you enter is public when you submit.")
        .accessibilityIdentifier("tera.add.disclosure.location")
      Text("Include an exact address only if you intend to share it publicly.")
      Text("Tera does not request your device location.")
      Text("You can enter a less precise description or leave optional location blank.")
      Text("Save draft keeps editing on this device.")
      Text("Submission and retraction cannot guarantee that remote copies will be removed.")
    }
  }
}
