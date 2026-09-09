import SwiftUI

struct TeraTodayPagingStatusView: View {
  @ObservedObject var store: TeraTodayStore

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      TeraTodayStatusView(presentation: store.presentation)
      if store.presentation.readFailure?.requiresRefresh == true {
        Button("Refresh posts") { Task { await store.reload() } }
          .buttonStyle(.bordered)
          .frame(minHeight: 44)
          .accessibilityHint("Loads the latest posts.")
          .accessibilityIdentifier("tera.today.restart")
      }
    }
  }
}
