import SwiftUI
import UIKit

struct RadrootsTodayView: View {
  let snapshot: RadrootsRuntimeSnapshot
  @ObservedObject var store: RadrootsTodayStore
  @ObservedObject var searchStore: RadrootsSearchStore
  @ObservedObject var meStore: RadrootsMeStore
  @ObservedObject var addStore: RadrootsAddStore
  @ObservedObject var settingsStore: RadrootsSettingsStore
  @ObservedObject var mediaStore: RadrootsMediaStore
  let revise: (RadrootsTodayCard) -> Void
  let retract: (RadrootsTodayCard) -> Void
  @State private var showsAccount = false
  @State private var showsContextPicker = false
  @State private var showsSearch = false

  var body: some View {
    Group {
      switch store.state {
      case .idle where store.cards.isEmpty,
        .loading where store.cards.isEmpty:
        ProgressView("Loading Today…")
          .accessibilityIdentifier("radroots.today.loading")
      case .empty:
        emptyView
      case .failed(let message) where store.cards.isEmpty:
        unavailableView(
          title: "Today is unavailable",
          message: message,
          systemImage: "exclamationmark.triangle"
        )
        .accessibilityIdentifier("radroots.today.error")
      case .offline(let message) where store.cards.isEmpty:
        unavailableView(
          title: "You’re offline",
          message: message,
          systemImage: "wifi.slash"
        )
        .accessibilityIdentifier("radroots.today.offline")
      default:
        feed
      }
    }
    .navigationTitle("Today")
    .toolbar { toolbarContent }
    .sheet(isPresented: $showsContextPicker) {
      RadrootsContextPicker(store: store)
    }
    .sheet(isPresented: $showsAccount, onDismiss: { meStore.stop() }) {
      RadrootsMeSheet(
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
      RadrootsSearchSheet(
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

  private var emptyView: some View {
    ScrollView {
      VStack(spacing: 16) {
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
          .accessibilityIdentifier("radroots.today.refresh")
      }
      .frame(maxWidth: .infinity)
      .padding()
    }
  }

  private var feed: some View {
    List {
      if case .offline(let message) = store.state {
        statusBanner(message: message, systemImage: "wifi.slash")
      } else if case .failed(let message) = store.state {
        statusBanner(message: message, systemImage: "exclamationmark.triangle")
      }

      ForEach(store.cards) { card in
        NavigationLink(value: card) {
          RadrootsTodayCardView(
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
    }
    .listStyle(.plain)
    .refreshable { await store.reload() }
    .navigationDestination(for: RadrootsTodayCard.self) { card in
      RadrootsTodayDetailView(
        card: card,
        context: store.selectedContext,
        mediaStore: mediaStore,
        canRevise: card.authorPublicKey == snapshot.identity.publicKeyHex
          && card.localOperationID != nil,
        canRetract: card.authorPublicKey == snapshot.identity.publicKeyHex,
        revise: revise,
        retract: retract
      )
    }
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

  private func unavailableView(
    title: String,
    message: String,
    systemImage: String
  ) -> some View {
    ContentUnavailableView {
      Label(title, systemImage: systemImage)
    } description: {
      Text(message)
    } actions: {
      Button("Try again") { Task { await store.reload() } }
    }
  }

  private func statusBanner(message: String, systemImage: String) -> some View {
    Label(message, systemImage: systemImage)
      .font(.footnote)
      .foregroundStyle(.secondary)
      .accessibilityIdentifier("radroots.today.status")
  }
}

private struct RadrootsContextPicker: View {
  @ObservedObject var store: RadrootsTodayStore
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      List(store.contexts) { context in
        Button {
          store.selectContext(id: context.id)
          dismiss()
        } label: {
          HStack {
            VStack(alignment: .leading, spacing: 4) {
              Text(context.label)
                .foregroundStyle(.primary)
              if let locality = context.locality {
                Text(locality)
                  .font(.caption)
                  .foregroundStyle(.secondary)
              }
            }
            Spacer()
            if context.id == store.selectedContextID {
              Image(systemName: "checkmark")
                .accessibilityHidden(true)
            }
          }
        }
        .accessibilityLabel(context.label)
        .accessibilityValue(context.id == store.selectedContextID ? "Selected" : "")
        .accessibilityIdentifier("radroots.context.\(context.id)")
      }
      .navigationTitle("Local network")
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button("Done") { dismiss() }
        }
      }
    }
    .presentationDetents([.medium, .large])
    .accessibilityIdentifier("radroots.context.picker")
  }
}

struct RadrootsTodayCardView: View {
  let card: RadrootsTodayCard
  let context: RadrootsLocalNetwork?
  @ObservedObject var mediaStore: RadrootsMediaStore

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack(alignment: .firstTextBaseline) {
        Text(card.authorName)
          .font(.subheadline.weight(.semibold))
        Spacer()
        Text(card.type.label)
          .font(.caption.weight(.semibold))
          .padding(.horizontal, 8)
          .padding(.vertical, 4)
          .background(.tint.opacity(0.12), in: Capsule())
      }

      if let title = card.title {
        Text(title)
          .font(.headline)
      }
      if !card.content.isEmpty {
        Text(card.content)
          .font(card.type == .ask ? .headline : .body)
      }

      if card.type == .event {
        eventMetadata
      }
      if card.type == .foodAvailability {
        foodMetadata
      }

      ForEach(card.media) { media in
        RadrootsTrustedMediaView(media: media, context: context, store: mediaStore)
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
    .accessibilityLabel(card.accessibilitySummary)
  }

  private var eventMetadata: some View {
    VStack(alignment: .leading, spacing: 4) {
      if let start = card.eventStartUnixSeconds {
        Label(
          Date(timeIntervalSince1970: TimeInterval(start)).formatted(
            date: .abbreviated, time: .shortened
          ),
          systemImage: "calendar"
        )
      }
      if let location = card.location {
        Label(location, systemImage: "mappin.and.ellipse")
      }
    }
    .font(.subheadline)
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
      Label(price, systemImage: "tag")
    }
    if let quantity = card.quantity, let unit = card.priceUnit {
      Label("\(quantity) \(unit) available", systemImage: "basket")
    }
    if let location = card.location {
      Label(location, systemImage: "mappin.and.ellipse")
    }
  }
}

struct RadrootsTrustedMediaView: View {
  let media: RadrootsMediaReference
  let context: RadrootsLocalNetwork?
  @ObservedObject var store: RadrootsMediaStore

  var body: some View {
    RadrootsLocalMediaContent(media: media, context: context, store: store)
      .frame(maxWidth: .infinity, minHeight: 120, maxHeight: 260)
      .background(.quaternary, in: RoundedRectangle(cornerRadius: 12))
      .clipShape(RoundedRectangle(cornerRadius: 12))
  }
}

struct RadrootsLocalMediaContent: View {
  let media: RadrootsMediaReference
  let context: RadrootsLocalNetwork?
  @ObservedObject var store: RadrootsMediaStore

  var body: some View {
    let state = store.state(for: media, context: context)
    Group {
      switch state {
      case .ready(let artifact):
        if let image = UIImage(data: artifact.bytes) {
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
      case .offline:
        mediaState("Photo unavailable offline", systemImage: "wifi.slash", retries: true)
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
    .task(id: media.id) {
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

  private func accessibilityLabel(for state: RadrootsMediaPresentationState) -> String {
    guard let alt = media.alt, !alt.isEmpty else { return state.accessibilityLabel }
    return "\(alt). \(state.accessibilityLabel)"
  }
}

struct RadrootsTodayDetailView: View {
  let card: RadrootsTodayCard
  let context: RadrootsLocalNetwork?
  @ObservedObject var mediaStore: RadrootsMediaStore
  let canRevise: Bool
  let canRetract: Bool
  let revise: (RadrootsTodayCard) -> Void
  let retract: (RadrootsTodayCard) -> Void
  @State private var showsRetractionConfirmation = false

  var body: some View {
    List {
      Section {
        RadrootsTodayCardView(card: card, context: context, mediaStore: mediaStore)
      }
      if !card.thread.isEmpty {
        Section("Conversation") {
          ForEach(card.thread) { entry in
            VStack(alignment: .leading, spacing: 4) {
              Text(entry.authorProfile?.preferredName ?? entry.authorPublicKey)
                .font(.subheadline.weight(.semibold))
              Text(entry.content)
              Text(entry.type.rawValue.capitalized)
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
          }
        }
      }
    }
    .navigationTitle(card.type.label)
    .navigationBarTitleDisplayMode(.inline)
    .toolbar {
      if canRevise {
        ToolbarItem(placement: .topBarTrailing) {
          Button("Revise") { revise(card) }
            .accessibilityIdentifier("radroots.today.revise")
        }
      }
      if canRetract {
        ToolbarItem(placement: .topBarTrailing) {
          Button("Retract", role: .destructive) { showsRetractionConfirmation = true }
            .accessibilityIdentifier("radroots.today.retract")
        }
      }
    }
    .confirmationDialog(
      "Retract this post?",
      isPresented: $showsRetractionConfirmation,
      titleVisibility: .visible
    ) {
      Button("Retract post", role: .destructive) { retract(card) }
      Button("Keep post", role: .cancel) {}
    } message: {
      Text("A signed retraction will be saved to the durable outbox before relay delivery.")
    }
    .accessibilityIdentifier("radroots.today.detail.\(card.id)")
  }
}
