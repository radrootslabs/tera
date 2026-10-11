import SwiftUI

/// Navigation holds identity and context, while the observed owner supplies
/// the current value. Mandatory invalidation removes even an open detail.
struct TeraSearchCurrentDetail: View {
  let resultID: String
  let type: TeraSearchResultType
  @ObservedObject var store: TeraSearchStore
  let context: TeraLocalNetwork?
  @ObservedObject var mediaStore: TeraMediaStore
  let publicKey: String
  let revise: (TeraTodayCard) -> Void
  let retract: (TeraTodayCard) -> Void

  var body: some View {
    if let result = store.currentResult(id: resultID, type: type, context: context) {
      if let card = result.card {
        TeraTodayDetailView(
          card: card, context: context, mediaStore: mediaStore,
          canRevise: card.authorPublicKey == publicKey && card.localOperationID != nil,
          canRetract: card.authorPublicKey == publicKey, revise: revise, retract: retract
        )
      } else if let profile = result.profile {
        TeraProfileView(profile: profile, context: context, mediaStore: mediaStore)
      } else {
        TeraSupportingUnavailableDetail()
      }
    } else {
      TeraSupportingUnavailableDetail()
    }
  }
}

struct TeraMeCurrentDetail: View {
  let cardID: String?
  @ObservedObject var store: TeraMeStore
  let context: TeraLocalNetwork?
  @ObservedObject var mediaStore: TeraMediaStore
  let publicKey: String
  let revise: (TeraTodayCard) -> Void
  let retract: (TeraTodayCard) -> Void

  var body: some View {
    if let cardID {
      if let card = store.currentCard(id: cardID, context: context) {
        TeraTodayDetailView(
          card: card, context: context, mediaStore: mediaStore,
          canRevise: card.authorPublicKey == publicKey && card.localOperationID != nil,
          canRetract: card.authorPublicKey == publicKey, revise: revise, retract: retract
        )
      } else {
        TeraSupportingUnavailableDetail()
      }
    } else if let profile = store.currentProfile(context: context) {
      TeraProfileView(profile: profile, context: context, mediaStore: mediaStore)
    } else {
      TeraSupportingUnavailableDetail()
    }
  }
}

private struct TeraSupportingUnavailableDetail: View {
  var body: some View {
    ContentUnavailableView(
      "Content unavailable", systemImage: "eye.slash",
      description: Text("This content is no longer available in this local network.")
    )
    .accessibilityIdentifier("tera.supporting.detail.unavailable")
  }
}
