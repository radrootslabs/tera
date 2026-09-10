import SwiftUI

/// Navigation retains identity; the visible value always comes from the store.
struct TeraTodayCurrentDetail: View {
  let cardID: String
  @ObservedObject var store: TeraTodayStore
  let context: TeraLocalNetwork?
  @ObservedObject var mediaStore: TeraMediaStore
  let canRevise: Bool
  let canRetract: Bool
  let revise: (TeraTodayCard) -> Void
  let retract: (TeraTodayCard) -> Void

  var body: some View {
    if let card = store.currentCard(id: cardID) {
      TeraTodayDetailView(
        card: card, context: context, mediaStore: mediaStore,
        canRevise: canRevise && card.localOperationID != nil, canRetract: canRetract,
        revise: revise, retract: retract
      )
    } else {
      ContentUnavailableView(
        "Post unavailable", systemImage: "eye.slash",
        description: Text("This post is no longer available in this local network.")
      )
      .accessibilityIdentifier("tera.today.detail.unavailable")
    }
  }
}
