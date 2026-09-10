import SwiftUI
import UIKit

struct TeraTodayView: View {
  let snapshot: TeraRuntimeSnapshot
  @ObservedObject var store: TeraTodayStore
  @ObservedObject var searchStore: TeraSearchStore
  @ObservedObject var meStore: TeraMeStore
  @ObservedObject var addStore: TeraAddStore
  @ObservedObject var settingsStore: TeraSettingsStore
  @ObservedObject var mediaStore: TeraMediaStore
  let revise: (TeraTodayCard) -> Void
  let retract: (TeraTodayCard) -> Void
  @State private var showsAccount = false
  @State private var showsContextPicker = false
  @State private var showsSearch = false

  var body: some View {
    Group {
      switch store.presentation.content {
      case .notLoaded:
        initialContent
      case .empty:
        emptyView
      case .available:
        feed
      }
    }
    .navigationTitle("Today")
    .toolbar { toolbarContent }
    .sheet(isPresented: $showsContextPicker) {
      TeraContextPicker(store: store)
    }
    .sheet(isPresented: $showsAccount, onDismiss: { meStore.stop() }) {
      TeraMeSheet(
        runtimeSnapshot: snapshot,
        context: store.selectedContext,
        store: meStore,
        todayStore: store,
        addStore: addStore,
        settingsStore: settingsStore,
        mediaStore: mediaStore,
        revise: revise,
        retract: retract
      )
    }
    .sheet(isPresented: $showsSearch, onDismiss: { searchStore.stop() }) {
      TeraSearchSheet(
        snapshot: snapshot,
        context: store.selectedContext,
        store: searchStore,
        mediaStore: mediaStore,
        revise: revise,
        retract: retract
      )
    }
    .task { await store.start() }
  }

  @ViewBuilder
  private var initialContent: some View {
    if let failure = store.presentation.readFailure {
      unavailableView(failure)
        .accessibilityIdentifier("radroots.today.error")
    } else {
      TeraTodayStatusView(presentation: store.presentation)
        .accessibilityIdentifier("radroots.today.loading")
    }
  }

  private var emptyView: some View {
    ScrollView {
      VStack(spacing: 16) {
        TeraTodayStatusView(presentation: store.presentation)
        TeraTodayDiscoveryView(store: store)
        Image(systemName: "leaf")
          .font(.largeTitle)
          .foregroundStyle(.secondary)
          .accessibilityHidden(true)
        Text("Nothing here yet")
          .font(.title2.weight(.semibold))
          .multilineTextAlignment(.center)
        Text("Pull to refresh or add the first update to this local network.")
          .multilineTextAlignment(.center)
        Button("Refresh") { Task { await store.reload() } }
          .buttonStyle(.borderedProminent)
          .tint(.primary)
          .frame(minWidth: 44, minHeight: 44)
          .contentShape(Rectangle())
          .accessibilityIdentifier("radroots.today.refresh.empty")
      }
      .frame(maxWidth: .infinity)
      .padding()
    }
  }

  private var feed: some View {
    List {
      TeraTodayPagingStatusView(store: store)

      ForEach(store.cards) { card in
        NavigationLink(value: card) {
          TeraTodayCardView(
            card: card,
            context: store.selectedContext,
            mediaStore: mediaStore
          )
        }
        .accessibilityIdentifier("radroots.today.card.\(card.id)")
        .onAppear {
          guard card.id == store.cards.last?.id, store.canLoadNextPage else { return }
          Task { await store.loadNextPage() }
        }
      }

      if store.isLoadingNextPage {
        HStack {
          Spacer()
          ProgressView("Loading more…")
          Spacer()
        }
        .accessibilityIdentifier("radroots.today.loading_more")
      }
      TeraTodayDiscoveryView(store: store)
    }
    .listStyle(.plain)
    .refreshable { await store.reload() }
    .navigationDestination(for: TeraTodayCard.self) { card in
      TeraTodayCurrentDetail(
        cardID: card.id, store: store,
        context: store.selectedContext,
        mediaStore: mediaStore,
        canRevise: card.authorPublicKey == snapshot.identity.publicKeyHex
          && card.localOperationID != nil,
        canRetract: card.authorPublicKey == snapshot.identity.publicKeyHex,
        revise: revise,
        retract: retract
      )
    }
    .environment(\.timeZone, store.viewerCalendar?.timeZone ?? .current)
    .accessibilityIdentifier("radroots.today.feed")
  }

  @ToolbarContentBuilder
  private var toolbarContent: some ToolbarContent {
    ToolbarItem(placement: .topBarLeading) {
      Button {
        showsContextPicker = true
      } label: {
        Label(store.selectedContext?.label ?? "Local network", systemImage: "location.circle")
      }
      .accessibilityLabel("Choose local network")
      .accessibilityValue(store.selectedContext?.label ?? "None")
      .accessibilityIdentifier("radroots.support.context")
    }
    ToolbarItem(placement: .topBarTrailing) {
      Button {
        Task { await store.reload() }
      } label: {
        Label("Refresh", systemImage: "arrow.clockwise")
      }
      .accessibilityIdentifier("radroots.today.refresh")
    }
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

  private func unavailableView(_ failure: TeraTodayFailure) -> some View {
    ContentUnavailableView {
      Label("Today is unavailable", systemImage: failure.systemImage)
    } description: {
      TeraTodayStatusView(presentation: store.presentation)
    } actions: {
      Button("Try again") { Task { await store.reload() } }
    }
  }
}

struct TeraTodayCardView: View {
  @Environment(\.locale) private var locale
  @Environment(\.timeZone) private var timeZone
  let card: TeraTodayCard
  let context: TeraLocalNetwork?
  @ObservedObject var mediaStore: TeraMediaStore

  var presentation: TeraTodayCardPresentation = .feed

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack(alignment: .firstTextBaseline) {
        Text(presentation.label(card.authorName))
          .font(.subheadline.weight(.semibold))
        Spacer()
        Text(card.type.label)
          .font(.caption.weight(.semibold))
          .padding(.horizontal, 8)
          .padding(.vertical, 4)
          .background(.tint.opacity(0.12), in: Capsule())
      }

      if let title = card.title {
        Text(presentation.label(title))
          .font(.headline)
      }
      if !card.content.isEmpty {
        Text(presentation.content(card.content))
          .font(card.type == .ask ? .headline : .body)
      }

      if card.type == .event {
        TeraCalendarMetadata(card: card, presentation: presentation)
      }
      if card.type == .foodAvailability {
        foodMetadata
      }

      ForEach(presentation.media(card.media)) { media in
        TeraTrustedMediaView(media: media, context: context, store: mediaStore)
      }

      HStack(spacing: 12) {
        Text(
          Date(timeIntervalSince1970: TimeInterval(card.authoredAtUnixSeconds)), style: .relative
        )
        if card.lifecycle != .active {
          Label(card.lifecycle.rawValue.capitalized, systemImage: "clock")
        }
        if let operationState = card.localOperationState {
          Label(
            operationState.replacingOccurrences(of: "_", with: " ").capitalized,
            systemImage: "arrow.triangle.2.circlepath"
          )
        }
      }
      .font(.caption)
      .foregroundStyle(.secondary)
    }
    .padding(.vertical, 8)
    .accessibilityElement(children: .combine)
    .accessibilityLabel(presentation.accessibility(card, locale: locale, timeZone: timeZone))
  }

  private var foodMetadata: some View {
    ViewThatFits(in: .horizontal) {
      HStack(spacing: 12) {
        foodMetadataLabels
      }
      VStack(alignment: .leading, spacing: 4) {
        foodMetadataLabels
      }
    }
    .font(.subheadline)
  }

  @ViewBuilder
  private var foodMetadataLabels: some View {
    if let price = card.priceSummary {
      Label(presentation.label(price), systemImage: "tag")
    }
    if let quantity = card.quantity, let unit = card.priceUnit {
      Label(presentation.label("\(quantity) \(unit) available"), systemImage: "basket")
    }
    if let location = card.location {
      Label(presentation.label(location), systemImage: "mappin.and.ellipse")
    }
  }
}

struct TeraTrustedMediaView: View {
  let media: TeraMediaReference
  let context: TeraLocalNetwork?
  @ObservedObject var store: TeraMediaStore

  var body: some View {
    TeraLocalMediaContent(media: media, context: context, store: store)
      .frame(maxWidth: .infinity, minHeight: 120, maxHeight: 260)
      .background(.quaternary, in: RoundedRectangle(cornerRadius: 12))
      .clipShape(RoundedRectangle(cornerRadius: 12))
  }
}

struct TeraLocalMediaContent: View {
  let media: TeraMediaReference
  let context: TeraLocalNetwork?
  @ObservedObject var store: TeraMediaStore

  var body: some View {
    let state = store.state(for: media, context: context)
    Group {
      switch state {
      case .ready:
        if let image = store.image(for: media, context: context) {
          Image(uiImage: image)
            .resizable()
            .scaledToFill()
        } else {
          mediaState("Saved photo is corrupt", systemImage: "shield.slash", retries: true)
        }
      case .pending:
        mediaState("Verifying photo", systemImage: "hourglass", retries: false)
      case .loading:
        ProgressView("Loading verified photo…")
      case .unavailable:
        mediaState("Photo is not available locally", systemImage: "photo", retries: true)
      case .networkUnavailable:
        mediaState("Photo service unavailable", systemImage: "wifi.slash", retries: true)
      case .corrupt:
        mediaState("Saved photo failed verification", systemImage: "shield.slash", retries: true)
      case .failed:
        mediaState(
          "Photo could not be loaded",
          systemImage: "photo.badge.exclamationmark",
          retries: true
        )
      }
    }
    .task(id: TeraMediaStore.Request(referenceID: media.id, context: context)) {
      store.load(media: media, context: context)
    }
    .accessibilityElement(children: .combine)
    .accessibilityLabel(accessibilityLabel(for: state))
  }

  private func mediaState(
    _ message: String,
    systemImage: String,
    retries: Bool
  ) -> some View {
    VStack(spacing: 8) {
      Label(message, systemImage: systemImage)
      if retries, context != nil {
        Button("Retry photo") {
          store.retry(media: media, context: context)
        }
        .buttonStyle(.bordered)
      }
    }
    .font(.caption)
  }

  private func accessibilityLabel(for state: TeraMediaPresentationState) -> String {
    guard let alt = media.alt, !alt.isEmpty else { return state.accessibilityLabel }
    return "\(TeraTodayCardPresentation.feed.label(alt)). \(state.accessibilityLabel)"
  }
}
