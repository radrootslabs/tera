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
    let todayStore: RadrootsTodayStore?
    let addStore: RadrootsAddStore?
    let searchStore: RadrootsSearchStore?
    let meStore: RadrootsMeStore?
    let settingsStore: RadrootsSettingsStore?
    let mediaStore: RadrootsMediaStore?
    @SceneStorage("radroots.selected_root_tab") private var storedSelection = RadrootsRootTab.today.rawValue

    init(
        snapshot: RadrootsRuntimeSnapshot,
        todayStore: RadrootsTodayStore? = nil,
        addStore: RadrootsAddStore? = nil,
        searchStore: RadrootsSearchStore? = nil,
        meStore: RadrootsMeStore? = nil,
        settingsStore: RadrootsSettingsStore? = nil,
        mediaStore: RadrootsMediaStore? = nil
    ) {
        self.snapshot = snapshot
        self.todayStore = todayStore
        self.addStore = addStore
        self.searchStore = searchStore
        self.meStore = meStore
        self.settingsStore = settingsStore
        self.mediaStore = mediaStore
    }

    var body: some View {
        TabView(selection: selection) {
            NavigationStack {
                if let todayStore, let addStore, let searchStore, let meStore, let settingsStore,
                   let mediaStore
                {
                    RadrootsTodayView(
                        snapshot: snapshot,
                        store: todayStore,
                        searchStore: searchStore,
                        meStore: meStore,
                        addStore: addStore,
                        settingsStore: settingsStore,
                        mediaStore: mediaStore,
                        revise: { card in
                            Task {
                                await addStore.retractAndRevise(card)
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
                if let addStore {
                    RadrootsAddView(store: addStore)
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
