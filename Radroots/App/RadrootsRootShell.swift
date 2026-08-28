import SwiftUI

enum RadrootsRootTab: String, CaseIterable, Sendable {
  case today
  case add

  static func resolve(_ rawValue: String?) -> Self {
    guard let rawValue, let tab = Self(rawValue: rawValue) else { return .today }
    return tab
  }

  static func resolve(url: URL) -> Self? {
    guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
      components.scheme?.lowercased() == "radroots",
      components.user == nil,
      components.password == nil,
      components.port == nil,
      components.query == nil,
      components.fragment == nil,
      components.percentEncodedPath.isEmpty,
      let encodedHost = components.percentEncodedHost,
      !encodedHost.contains("%")
    else {
      return nil
    }
    return Self(rawValue: encodedHost.lowercased())
  }
}

struct RadrootsRootShell: View {
  let snapshot: RadrootsRuntimeSnapshot
  let stores: RadrootsProductStores?
  @SceneStorage("radroots.selected_root_tab") private var storedSelection = RadrootsRootTab.today
    .rawValue

  init(
    snapshot: RadrootsRuntimeSnapshot,
    stores: RadrootsProductStores? = nil
  ) {
    self.snapshot = snapshot
    self.stores = stores
  }

  var body: some View {
    TabView(selection: selection) {
      NavigationStack {
        if let stores {
          RadrootsTodayView(
            snapshot: snapshot,
            store: stores.today,
            searchStore: stores.search,
            meStore: stores.me,
            addStore: stores.add,
            settingsStore: stores.settings,
            mediaStore: stores.media,
            revise: { card in
              Task {
                await stores.add.retractAndRevise(card)
                storedSelection = RadrootsRootTab.add.rawValue
              }
            },
            retract: { card in
              Task {
                await stores.add.retract(card)
                storedSelection = RadrootsRootTab.add.rawValue
              }
            }
          )
        } else {
          RadrootsTodayLanding(snapshot: snapshot)
        }
      }
      .tabItem { Label("Today", systemImage: "sun.max.fill") }
      .tag(RadrootsRootTab.today)
      .accessibilityIdentifier("radroots.tab.today")

      NavigationStack {
        if let stores {
          RadrootsAddView(store: stores.add)
        } else {
          RadrootsAddUnavailable()
        }
      }
      .tabItem { Label("Add", systemImage: "plus.circle.fill") }
      .tag(RadrootsRootTab.add)
      .accessibilityIdentifier("radroots.tab.add")
    }
    .accessibilityIdentifier("radroots.root.tabs")
    .onOpenURL { url in
      guard let tab = RadrootsRootTab.resolve(url: url) else { return }
      storedSelection = tab.rawValue
    }
  }

  private var selection: Binding<RadrootsRootTab> {
    Binding(
      get: { RadrootsRootTab.resolve(storedSelection) },
      set: { storedSelection = $0.rawValue }
    )
  }
}

private struct RadrootsTodayLanding: View {
  let snapshot: RadrootsRuntimeSnapshot
  @State private var showsAccount = false
  @State private var showsSearch = false

  var body: some View {
    ContentUnavailableView {
      Label("Today", systemImage: "leaf.fill")
    } description: {
      Text("Your local food network is ready for its first refresh.")
    }
    .navigationTitle("Today")
    .toolbar {
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          showsSearch = true
        } label: {
          Label("Search", systemImage: "magnifyingglass")
        }
        .accessibilityIdentifier("radroots.support.search")
      }
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          showsAccount = true
        } label: {
          Label("Account", systemImage: "person.crop.circle")
        }
        .accessibilityIdentifier("radroots.support.account")
      }
    }
    .sheet(isPresented: $showsAccount) {
      RadrootsAccountUnavailableSheet(snapshot: snapshot)
    }
    .sheet(isPresented: $showsSearch) {
      RadrootsSearchUnavailableSheet()
    }
    .accessibilityIdentifier("radroots.today.root")
  }
}

struct RadrootsSearchUnavailableSheet: View {
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      ContentUnavailableView.search
        .navigationTitle("Search")
        .toolbar {
          ToolbarItem(placement: .confirmationAction) {
            Button("Done") { dismiss() }
          }
        }
    }
    .accessibilityIdentifier("radroots.support.search.sheet")
  }
}

struct RadrootsAccountUnavailableSheet: View {
  let snapshot: RadrootsRuntimeSnapshot
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      List {
        Section("Me") {
          LabeledContent("Public key", value: abbreviatedPublicKey)
        }
        Section("Connection") {
          LabeledContent("Runtime", value: snapshot.crateVersion)
          LabeledContent("Relay", value: snapshot.relay?.state ?? "Not configured")
        }
      }
      .navigationTitle("Account")
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button("Done") { dismiss() }
        }
      }
    }
    .presentationDetents([.medium, .large])
    .accessibilityIdentifier("radroots.support.account.sheet")
  }

  private var abbreviatedPublicKey: String {
    let key = snapshot.identity.publicKeyHex
    guard key.count > 16 else { return key }
    return "\(key.prefix(8))…\(key.suffix(8))"
  }
}

private struct RadrootsAddUnavailable: View {
  var body: some View {
    ContentUnavailableView {
      Label("Add", systemImage: "plus.circle.fill")
    } description: {
      Text("Choose what to share with your local food network.")
    }
    .navigationTitle("Add")
    .accessibilityIdentifier("radroots.add.root")
  }
}
