import SwiftUI

struct TeraTodayDetailView: View {
  let card: TeraTodayCard
  let context: TeraLocalNetwork?
  @ObservedObject var mediaStore: TeraMediaStore
  let canRevise: Bool
  let canRetract: Bool
  let revise: (TeraTodayCard) -> Void
  let retract: (TeraTodayCard) -> Void
  @State private var showsRetractionConfirmation = false

  var body: some View {
    List {
      Section {
        TeraTodayCardView(card: card, context: context, mediaStore: mediaStore, presentation: .detail)
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
