import SwiftUI

struct TeraTodayDiscoveryView: View {
  @ObservedObject var store: TeraTodayStore

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      if let message = store.discovery.message {
        Text(message).font(.footnote).foregroundStyle(.secondary)
      }
      if let failure = store.discovery.failure {
        Label(failure.message, systemImage: failure.systemImage)
          .font(.footnote)
      }
      if store.discovery.isSearching {
        ProgressView("Searching older posts…")
      } else if store.discovery.canSearchOlder {
        Button("Search older posts") { Task { await store.searchOlderPosts() } }
          .frame(minHeight: 44)
          .accessibilityIdentifier("tera.today.search_older")
      }
    }
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("tera.today.discovery")
  }
}
