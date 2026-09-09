import SwiftUI

/// A scope change also ends the lifetime of its details and supporting sheets.
struct TeraTodayNavigation: View {
  let snapshot: TeraRuntimeSnapshot
  let stores: TeraProductStores
  let selectAdd: () -> Void
  @ObservedObject private var today: TeraTodayStore

  init(snapshot: TeraRuntimeSnapshot, stores: TeraProductStores, selectAdd: @escaping () -> Void) {
    self.snapshot = snapshot
    self.stores = stores
    self.selectAdd = selectAdd
    today = stores.today
  }

  var body: some View {
    NavigationStack {
      TeraTodayView(
        snapshot: snapshot,
        store: today,
        searchStore: stores.search,
        meStore: stores.me,
        addStore: stores.add,
        settingsStore: stores.settings,
        mediaStore: stores.media,
        revise: { card in
          Task {
            await stores.add.retractAndRevise(card)
            selectAdd()
          }
        },
        retract: { card in
          Task {
            await stores.add.retract(card)
            selectAdd()
          }
        }
      )
    }
    .id(today.scopeGeneration)
  }
}
