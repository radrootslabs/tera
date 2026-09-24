import RadrootsKit
import SwiftUI

struct TeraSearchSheet: View {
  let snapshot: TeraRuntimeSnapshot
  let context: TeraLocalNetwork?
  @ObservedObject var store: TeraSearchStore
  @ObservedObject var mediaStore: TeraMediaStore
  let revise: (TeraTodayCard) -> Void
  let retract: (TeraTodayCard) -> Void
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      Group {
        switch store.state {
        case .idle:
          ContentUnavailableView(
            "Search your local network",
            systemImage: "magnifyingglass",
            description: Text("Find current posts and adopted Nostr profiles.")
          )
        case .loading:
          ProgressView("Searching…")
        case .empty:
          ContentUnavailableView.search(text: store.query)
        case let .failed(message):
          ContentUnavailableView {
            Label("Search unavailable", systemImage: "exclamationmark.triangle")
          } description: {
            Text(message)
          } actions: {
            Button("Try again") { Task { await store.search() } }
          }
        case .loaded:
          resultList
        }
      }
      .navigationTitle("Search")
      .searchable(text: query, prompt: "Posts and profiles")
      .onSubmit(of: .search) { Task { await store.search() } }
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button("Done") { dismiss() }
        }
      }
    }
    .task(id: context) { store.configure(context: context) }
    .accessibilityIdentifier("radroots.support.search.sheet")
  }

  private var resultList: some View {
    List(store.results) { result in
      switch (result.card, result.profile) {
      case let (card?, _):
        NavigationLink {
          TeraTodayDetailView(
            card: card,
            context: context,
            mediaStore: mediaStore,
            canRevise: card.authorPublicKey == snapshot.identity.publicKeyHex
              && card.localOperationID != nil,
            canRetract: card.authorPublicKey == snapshot.identity.publicKeyHex,
            revise: revise,
            retract: retract
          )
        } label: {
          TeraTodayCardView(card: card, context: context, mediaStore: mediaStore)
        }
        .accessibilityIdentifier("radroots.search.card.\(result.id)")
      case let (_, profile?):
        NavigationLink {
          TeraProfileView(profile: profile, context: context, mediaStore: mediaStore)
        } label: {
          TeraProfileRow(profile: profile, context: context, mediaStore: mediaStore)
        }
        .accessibilityIdentifier("radroots.search.profile.\(result.id)")
      default:
        EmptyView()
      }
    }
    .listStyle(.plain)
    .accessibilityIdentifier("radroots.search.results")
  }

  private var query: Binding<String> {
    Binding(
      get: { store.query },
      set: { value in store.updateQuery(value) }
    )
  }
}

struct TeraMeSheet: View {
  let runtimeSnapshot: TeraRuntimeSnapshot
  let context: TeraLocalNetwork?
  @ObservedObject var store: TeraMeStore
  @ObservedObject var todayStore: TeraTodayStore
  @ObservedObject var addStore: TeraAddStore
  @ObservedObject var settingsStore: TeraSettingsStore
  @ObservedObject var mediaStore: TeraMediaStore
  let revise: (TeraTodayCard) -> Void
  let retract: (TeraTodayCard) -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var showsDrafts = false

  var body: some View {
    NavigationStack {
      Group {
        switch store.state {
        case .idle, .loading:
          ProgressView("Loading your profile…")
        case let .failed(message) where store.snapshot == nil:
          ContentUnavailableView {
            Label("Profile unavailable", systemImage: "person.crop.circle.badge.exclamationmark")
          } description: {
            Text(message)
          } actions: {
            Button("Try again") { Task { await store.reload() } }
          }
        default:
          content
        }
      }
      .navigationTitle("Me")
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button("Done") { dismiss() }
        }
      }
    }
    .sheet(isPresented: $showsDrafts) {
      TeraDraftsSheet(store: addStore)
    }
    .task(id: context) {
      store.configure(context: context)
      await store.start()
    }
    .presentationDetents([.medium, .large])
    .accessibilityIdentifier("radroots.support.me.sheet")
  }

  private var content: some View {
    List {
      Section {
        if let profile = store.snapshot?.profile {
          NavigationLink {
            TeraProfileView(profile: profile, context: context, mediaStore: mediaStore)
          } label: {
            TeraProfileRow(profile: profile, context: context, mediaStore: mediaStore)
          }
        } else {
          TeraProfileRow(
            profile: TeraProfileSummary(
              authorPublicKey: store.snapshot?.publicKey ?? runtimeSnapshot.identity.publicKeyHex,
              name: nil,
              displayName: nil,
              about: nil,
              picture: nil,
              banner: nil,
              nip05: nil,
              website: nil,
              lightningAddress: nil
            ),
            context: context,
            mediaStore: mediaStore
          )
        }
      }

      Section("Local work") {
        Button {
          showsDrafts = true
        } label: {
          LabeledContent("Drafts & outbox", value: "\(addStore.drafts.count)")
        }
        .foregroundStyle(.primary)
        .accessibilityIdentifier("radroots.support.outbox")
      }

      Section("My posts") {
        if store.snapshot?.cards.isEmpty != false {
          Text("No current posts in this local network.")
            .foregroundStyle(.secondary)
        }
        ForEach(store.snapshot?.cards ?? []) { card in
          NavigationLink {
            TeraTodayDetailView(
              card: card,
              context: context,
              mediaStore: mediaStore,
              canRevise: card.localOperationID != nil,
              canRetract: card.authorPublicKey
                == (store.snapshot?.publicKey ?? runtimeSnapshot.identity.publicKeyHex),
              revise: revise,
              retract: retract
            )
          } label: {
            TeraTodayCardView(card: card, context: context, mediaStore: mediaStore)
          }
        }
      }

      Section {
        NavigationLink("Settings") {
          TeraSettingsView(
            snapshot: runtimeSnapshot,
            todayStore: todayStore,
            addStore: addStore,
            meStore: store,
            settingsStore: settingsStore
          )
        }
        .accessibilityIdentifier("radroots.support.settings")
      }
    }
    .refreshable { await store.reload() }
  }
}

struct TeraProfileRow: View {
  let profile: TeraProfileSummary
  let context: TeraLocalNetwork?
  @ObservedObject var mediaStore: TeraMediaStore

  var body: some View {
    HStack(spacing: 12) {
      TeraStableAvatarView(
        profile: profile,
        context: context,
        mediaStore: mediaStore,
        size: 48
      )
      VStack(alignment: .leading, spacing: 3) {
        Text(profile.preferredName)
          .font(.headline)
        if let nip05 = profile.nip05 {
          Text(nip05)
            .font(.caption)
            .foregroundStyle(.secondary)
        } else {
          Text(Self.abbreviate(profile.authorPublicKey))
            .font(.caption.monospaced())
            .foregroundStyle(.secondary)
        }
      }
    }
    .accessibilityElement(children: .combine)
    .accessibilityLabel("\(profile.preferredName), \(Self.abbreviate(profile.authorPublicKey))")
  }

  private static func abbreviate(_ key: String) -> String {
    guard key.count > 16 else { return key }
    return "\(key.prefix(8))…\(key.suffix(8))"
  }
}

struct TeraProfileView: View {
  let profile: TeraProfileSummary
  let context: TeraLocalNetwork?
  @ObservedObject var mediaStore: TeraMediaStore

  var body: some View {
    List {
      Section {
        VStack(spacing: 12) {
          if let banner = profile.banner {
            TeraLocalMediaContent(media: banner, context: context, store: mediaStore)
              .frame(maxWidth: .infinity, minHeight: 100, maxHeight: 160)
              .background(.quaternary, in: RoundedRectangle(cornerRadius: 12))
              .clipShape(RoundedRectangle(cornerRadius: 12))
          }
          TeraStableAvatarView(
            profile: profile,
            context: context,
            mediaStore: mediaStore,
            size: 88
          )
          Text(profile.preferredName)
            .font(.title2.weight(.semibold))
          Text(abbreviatedPublicKey)
            .font(.caption.monospaced())
            .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .combine)
      }
      if let about = profile.about, !about.isEmpty {
        Section("About") { Text(about) }
      }
      Section("Nostr profile") {
        if let nip05 = profile.nip05 {
          LabeledContent("NIP-05", value: nip05)
        }
        if let website = safeHTTPS(profile.website) {
          Link(destination: website) {
            LabeledContent("Website", value: website.host ?? website.absoluteString)
          }
        }
        if let address = profile.lightningAddress {
          LabeledContent("Lightning", value: address)
        }
      }
    }
    .navigationTitle("Profile")
    .navigationBarTitleDisplayMode(.inline)
    .accessibilityIdentifier("radroots.profile.\(profile.authorPublicKey)")
  }

  private var abbreviatedPublicKey: String {
    guard profile.authorPublicKey.count > 16 else { return profile.authorPublicKey }
    return "\(profile.authorPublicKey.prefix(8))…\(profile.authorPublicKey.suffix(8))"
  }

  private func safeHTTPS(_ value: String?) -> URL? {
    guard let value,
      let url = URL(string: value),
      url.scheme?.lowercased() == "https",
      url.host != nil,
      url.user == nil,
      url.password == nil
    else {
      return nil
    }
    return url
  }
}

struct TeraStableAvatarView: View {
  let profile: TeraProfileSummary
  let context: TeraLocalNetwork?
  @ObservedObject var mediaStore: TeraMediaStore
  let size: CGFloat

  var body: some View {
    Group {
      if let picture = profile.picture {
        TeraLocalMediaContent(media: picture, context: context, store: mediaStore)
      } else {
        fallback
      }
    }
    .frame(width: size, height: size)
    .background(.quaternary)
    .clipShape(Circle())
  }

  private var fallback: some View {
    let identity = TeraStableVisualIdentity(publicKeyHex: profile.authorPublicKey)
    return ZStack {
      Circle().fill(Self.palette[identity.paletteIndex])
      Text(String(profile.preferredName.prefix(1)).uppercased())
        .font(.system(size: size * 0.4, weight: .semibold, design: .rounded))
        .foregroundStyle(.white)
    }
    .accessibilityHidden(true)
  }

  private static let palette: [Color] = [
    .indigo, .mint, .orange, .purple, .teal, .pink,
    .blue, .green, .red, .cyan, .brown, .yellow,
  ]
}
