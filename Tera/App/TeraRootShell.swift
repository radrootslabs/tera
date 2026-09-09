import SwiftUI

enum TeraRootTab: String, CaseIterable, Sendable {
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

struct TeraRootShell: View {
  let snapshot: TeraRuntimeSnapshot
  let stores: TeraProductStores?
  @SceneStorage("radroots.selected_root_tab") private var storedSelection = TeraRootTab.today
    .rawValue

  init(
    snapshot: TeraRuntimeSnapshot,
    stores: TeraProductStores? = nil
  ) {
    self.snapshot = snapshot
    self.stores = stores
  }

  var body: some View {
    TabView(selection: selection) {
      Group {
        if let stores {
          TeraTodayNavigation(snapshot: snapshot, stores: stores) {
            storedSelection = TeraRootTab.add.rawValue
          }
        } else {
          NavigationStack { TeraTodayLanding(snapshot: snapshot) }
        }
      }
      .tabItem { Label("Today", systemImage: "sun.max.fill") }
      .tag(TeraRootTab.today)
      .accessibilityIdentifier("radroots.tab.today")

      NavigationStack {
        if let stores {
          TeraAddView(store: stores.add)
        } else {
          TeraAddUnavailable()
        }
      }
      .tabItem { Label("Add", systemImage: "plus.circle.fill") }
      .tag(TeraRootTab.add)
      .accessibilityIdentifier("radroots.tab.add")
    }
    .tint(.primary)
    .toolbarBackground(Color(uiColor: .systemBackground), for: .tabBar)
    .toolbarBackground(.visible, for: .tabBar)
    .accessibilityIdentifier("radroots.root.tabs")
    .onOpenURL { url in
      guard let tab = TeraRootTab.resolve(url: url) else { return }
      storedSelection = tab.rawValue
    }
  }

  private var selection: Binding<TeraRootTab> {
    Binding(
      get: { TeraRootTab.resolve(storedSelection) },
      set: { storedSelection = $0.rawValue }
    )
  }
}

private struct TeraTodayLanding: View {
  let snapshot: TeraRuntimeSnapshot
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
      TeraAccountUnavailableSheet(snapshot: snapshot)
    }
    .sheet(isPresented: $showsSearch) {
      TeraSearchUnavailableSheet()
    }
    .accessibilityIdentifier("radroots.today.root")
  }
}

struct TeraSearchUnavailableSheet: View {
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

struct TeraAccountUnavailableSheet: View {
  let snapshot: TeraRuntimeSnapshot
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

private struct TeraAddUnavailable: View {
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
