import SwiftUI
import UIKit

struct TeraAddView: View {
  private enum FocusedField: Hashable {
    case title
    case summary
    case content
    case location
    case price
    case currency
    case quantity
    case media(String)
  }

  @ObservedObject var store: TeraAddStore
  @Environment(\.dynamicTypeSize) private var dynamicTypeSize
  @State private var showsDrafts = false
  @FocusState private var focusedField: FocusedField?

  var body: some View {
    Form {
      switch store.state {
      case .idle, .loading:
        Section {
          ProgressView("Loading Add…")
            .accessibilityIdentifier("radroots.add.loading")
        }
      case let .failed(message):
        Section {
          Label(message, systemImage: "exclamationmark.triangle")
            .foregroundStyle(.secondary)
            .accessibilityIdentifier("radroots.add.error")
        }
      case .ready:
        EmptyView()
      }

      TeraAddSaveStatus(message: store.message, symbol: statusSymbol, state: store.composerState, mediaMessage: store.mediaRecoveryMessage, protection: store.protection)

      if store.activeDraft?.kind == .retraction {
        Section("Retraction") {
          Text("This saved operation removes one of your published posts.")
            .foregroundStyle(.secondary)
        }
      } else {
        Group {
          Section("What are you sharing?") {
            Picker("Type", selection: typeBinding) {
              ForEach(availableTypes) { type in
                Text(type.label).tag(type)
              }
            }
            .pickerStyle(.navigationLink)
            .foregroundStyle(.primary)
            .disabled(!store.isFormEditable)
            .accessibilityIdentifier("radroots.add.type")
          }

          composerFields

          if store.acceptsMedia {
            mediaSection
          }
        }
        .disabled(!store.isProductReady)
      }

      Section {
        Button("Save draft") { Task { await store.save() } }
          .buttonStyle(.borderedProminent)
          .tint(.primary)
          .disabled(!store.canSave)
          .accessibilityIdentifier("radroots.add.save")

        if let active = store.activeDraft, active.state.canCancel {
          Button("Cancel local work", role: .destructive) {
            Task { await store.cancel(active) }
          }
          .disabled(store.isWorking)
          .accessibilityIdentifier("radroots.add.cancel")
        }
        Text(
          "Submit saves an immutable local snapshot first. If signing, media, or a relay is unavailable, the saved operation remains available to retry."
        )
        .font(.footnote)
        .foregroundStyle(.primary)

        if dynamicTypeSize.isAccessibilitySize {
          submitButton
        }
      }
    }
    .headerProminence(.increased)
    .tint(.primary)
    .accessibilityIdentifier("radroots.add.root")
    .accessibilityValue(store.isWorking ? "Working" : "Ready")
    .safeAreaInset(edge: .bottom, spacing: 0) {
      if !dynamicTypeSize.isAccessibilitySize {
        submitButton
          .padding(.horizontal)
          .padding(.vertical, 8)
          .padding(.bottom, 72)
          .background(.bar)
      }
    }
    .disabled(store.isWorking)
    .overlay {
      if store.isWorking {
        ProgressView("Saving…")
          .padding()
          .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
          .accessibilityIdentifier("radroots.add.progress")
      }
    }
    .navigationTitle("Add")
    .toolbarBackground(Color(uiColor: .systemBackground), for: .navigationBar)
    .toolbarBackground(.visible, for: .navigationBar)
    .toolbar {
      ToolbarItem(placement: .topBarLeading) {
        Button("Drafts") { showsDrafts = true }
          .accessibilityIdentifier("radroots.add.drafts")
      }
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          store.newDraft()
        } label: {
          Label("New", systemImage: "square.and.pencil")
        }
        .accessibilityIdentifier("radroots.add.new")
      }
      ToolbarItemGroup(placement: .keyboard) {
        Spacer()
        Button("Done") { focusedField = nil }
          .accessibilityIdentifier("radroots.add.keyboard.done")
      }
    }
    .sheet(isPresented: $showsDrafts) {
      TeraDraftsSheet(store: store)
    }
    .task { await store.start() }
  }

  @ViewBuilder
  private var composerFields: some View {
    switch store.form.commandType {
    case .createUpdate:
      Section("Update") { contentEditor(prompt: "What’s happening locally?") }
    case .createPhotoUpdate:
      Section("Photo update") { contentEditor(prompt: "What should neighbors know?") }
    case .createAsk:
      Section("Ask") { contentEditor(prompt: "What do you need or want to know?") }
    case .createEvent:
      eventFields
    case .createFoodAvailability:
      foodFields
    }
  }

  private var eventFields: some View {
    Section("Event") {
      formTextField(
        label: "Title", text: optional(\.title), focus: .title,
        identifier: "radroots.add.title"
      )
      contentEditor(prompt: "Event details (optional)")
      formTextField(
        label: "Location (optional)", text: optional(\.location), focus: .location,
        identifier: "radroots.add.location"
      )
      TeraCalendarComposerFields(store: store)
    }
  }

  @ViewBuilder
  private var foodFields: some View {
    Section("Food availability") {
      formTextField(
        label: "Food", text: optional(\.title), focus: .title,
        identifier: "radroots.add.title"
      )
      formTextField(
        label: "Short summary", text: optional(\.summary), focus: .summary,
        identifier: "radroots.add.summary"
      )
      contentEditor(prompt: "Details")
      formTextField(
        label: "Location", text: optional(\.location), focus: .location,
        identifier: "radroots.add.location"
      )
    }
    Section("Price and quantity") {
      formTextField(
        label: "Price", text: optional(\.priceAmount), focus: .price,
        identifier: "radroots.add.price", keyboardType: .decimalPad
      )
      formTextField(
        label: "Currency", text: optional(\.currency), focus: .currency,
        identifier: "radroots.add.currency", capitalization: .characters
      )
      Picker("Unit", selection: optional(\.unit)) {
        Text("Choose a unit").tag("")
        ForEach(unitChoices, id: \.self) { unit in
          Text(unit).tag(unit)
        }
      }
      .accessibilityIdentifier("radroots.add.unit")
      formTextField(
        label: "Quantity available (optional)", text: optional(\.quantity), focus: .quantity,
        identifier: "radroots.add.quantity", keyboardType: .decimalPad
      )
    }
  }

  private var mediaSection: some View {
    Section("Photos") {
      ForEach(store.form.media) { media in
        VStack(alignment: .leading, spacing: 8) {
          Label("Prepared image", systemImage: "checkmark.shield")
            .font(.subheadline.weight(.semibold))
          Text(
            "\(media.width) × \(media.height) · \(ByteCountFormatter.string(fromByteCount: Int64(clamping: media.byteSize), countStyle: .file))"
          )
          .font(.caption)
          .foregroundStyle(.secondary)
          TextField(
            "Describe this photo",
            text: Binding(
              get: { media.alt },
              set: { store.updateMediaAlt(id: media.id, alt: $0) }
            )
          )
          .focused($focusedField, equals: .media(media.id))
          .accessibilityIdentifier("radroots.add.media.alt")
          Button("Remove photo", role: .destructive) { store.removeMedia(id: media.id) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("radroots.add.media.prepared")
        .accessibilityValue(
          "\(media.width) by \(media.height), "
            + ByteCountFormatter.string(
              fromByteCount: Int64(clamping: media.byteSize), countStyle: .file
            )
        )
      }

      ViewThatFits(in: .horizontal) {
        HStack {
          mediaLibraryButton
          Spacer()
          mediaCameraButton
        }
        VStack(alignment: .leading, spacing: 12) {
          mediaLibraryButton
          mediaCameraButton
        }
      }
    }
  }

  private var mediaLibraryButton: some View {
    Button {
      Task { await store.importPhotos() }
    } label: {
      HStack(alignment: .firstTextBaseline) {
        Image(systemName: "photo.on.rectangle")
        Text("Library")
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .disabled(!store.mediaSupport.library || !store.canAddMedia)
    .accessibilityLabel("Photo Library")
    .accessibilityIdentifier("radroots.add.media.library")
  }

  private var mediaCameraButton: some View {
    Button {
      Task { await store.capturePhoto() }
    } label: {
      HStack(alignment: .firstTextBaseline) {
        Image(systemName: "camera")
        Text("Camera")
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .disabled(!store.mediaSupport.camera || !store.canAddMedia)
    .accessibilityIdentifier("radroots.add.media.camera")
  }

  private func contentEditor(prompt: LocalizedStringKey) -> some View {
    ZStack(alignment: .topLeading) {
      TextEditor(text: required(\.content))
        .frame(minHeight: 110)
        .focused($focusedField, equals: .content)
        .accessibilityLabel(Text(prompt))
        .accessibilityIdentifier("radroots.add.content")
      if store.form.content.isEmpty {
        Text(prompt)
          .foregroundStyle(.primary)
          .padding(.horizontal, 5)
          .padding(.vertical, 8)
          .allowsHitTesting(false)
          .accessibilityHidden(true)
      }
    }
  }

  private var submitButton: some View {
    Button {
      Task { await store.submit() }
    } label: {
      Text(submitLabel)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity)
    }
    .accessibilityIdentifier("radroots.add.submit")
    .accessibilityValue(submitAccessibilityValue)
    .buttonStyle(.borderedProminent)
    .tint(.primary)
    .disabled(!store.canSubmit)
  }

  private func formTextField(
    label: LocalizedStringKey,
    text: Binding<String>,
    focus: FocusedField,
    identifier: String,
    keyboardType: UIKeyboardType = .default,
    capitalization: TextInputAutocapitalization? = nil
  ) -> some View {
    VStack(alignment: .leading, spacing: 6) {
      Text(label)
        .font(.subheadline.weight(.semibold))
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityHidden(true)
      TextField("", text: text)
        .keyboardType(keyboardType)
        .textInputAutocapitalization(capitalization)
        .focused($focusedField, equals: focus)
        .accessibilityLabel(Text(label))
        .accessibilityIdentifier(identifier)
    }
  }

  private var availableTypes: [TeraAddCommandType] {
    let types = store.schemas.map(\.commandType)
    return types.isEmpty ? TeraAddCommandType.allCases : types
  }

  private var unitChoices: [String] {
    store.selectedSchema?.fields.first(where: { $0.id == "unit" })?.choices ?? []
  }

  private var submitAccessibilityValue: String {
    if store.isWorking {
      return "Working"
    }
    if let message = store.message {
      if let code = store.lastFailureCode {
        return "\(message) Error code \(code)"
      }
      return message
    }
    if let activeDraft = store.activeDraft {
      return activeDraft.honestSummary
    }
    return store.canSubmit ? "Ready" : "Unavailable"
  }

  private var typeBinding: Binding<TeraAddCommandType> {
    Binding(get: { store.form.commandType }, set: { store.selectType($0) })
  }

  private func optional(_ keyPath: WritableKeyPath<TeraAddForm, String?>) -> Binding<String> {
    Binding(
      get: { store.form[keyPath: keyPath] ?? "" },
      set: { store.updateForm(keyPath, $0.isEmpty ? nil : $0) }
    )
  }

  private func required(_ keyPath: WritableKeyPath<TeraAddForm, String>) -> Binding<String> {
    Binding(
      get: { store.form[keyPath: keyPath] },
      set: { store.updateForm(keyPath, $0) }
    )
  }

  private var submitLabel: String {
    if store.activeDraft?.kind == .retraction {
      return "Retry retraction"
    }
    if let state = store.activeDraft?.state, state.canAdvance {
      return "Retry delivery"
    }
    return "Submit"
  }

  private var statusSymbol: String {
    switch store.activeDraft?.state {
    case .complete: "checkmark.circle.fill"
    case .partiallyDelivered, .retryable: "exclamationmark.arrow.triangle.2.circlepath"
    case .terminal: "xmark.octagon"
    case .cancelled: "slash.circle"
    default: "info.circle"
    }
  }
}
