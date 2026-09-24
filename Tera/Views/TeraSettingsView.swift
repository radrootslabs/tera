import SwiftUI

struct TeraSettingsView: View {
  let snapshot: TeraRuntimeSnapshot
  @ObservedObject var todayStore: TeraTodayStore
  @ObservedObject var addStore: TeraAddStore
  @ObservedObject var meStore: TeraMeStore
  @ObservedObject var settingsStore: TeraSettingsStore
  @EnvironmentObject private var diagnosticsStore: TeraDiagnosticsStore
  @EnvironmentObject private var appModel: TeraAppModel

  var body: some View {
    List {
      Section("Identity") {
        LabeledContent(
          "Local signer",
          value: snapshot.identity.hostSignerConfigured ? "Ready" : "Needs attention"
        )
        LabeledContent("Public key", value: abbreviatedPublicKey)
          .accessibilityIdentifier("radroots.settings.identity.public_key")
          .accessibilityValue(snapshot.identity.publicKeyHex)
        if let identity = settingsStore.settings?.identity {
          LabeledContent(
            "Custody state",
            value: identity.lockState == .unlocked ? "Unlocked" : "Locked"
          )
          ForEach(identity.identities) { record in
            HStack {
              Text(abbreviated(record.publicKeyHex)).font(.caption.monospaced())
              Spacer()
              if record.id == identity.activeIdentityID {
                Text("Selected").foregroundStyle(.secondary)
              }
            }
          }
        }
        Button("Lock identity", role: .destructive) {
          Task { await appModel.lockIdentity() }
        }
        .accessibilityIdentifier("radroots.settings.identity.lock")
      }
      Section("Profile") {
        TextField("Name", text: $settingsStore.profileName)
          .textInputAutocapitalization(.never)
          .accessibilityIdentifier("radroots.settings.profile.name")
        TextField("Display name", text: $settingsStore.profileDisplayName)
          .accessibilityIdentifier("radroots.settings.profile.display_name")
        TextField("About", text: $settingsStore.profileAbout, axis: .vertical)
          .lineLimit(3 ... 8)
          .accessibilityIdentifier("radroots.settings.profile.about")
        TextField("NIP-05 identifier", text: $settingsStore.profileNip05)
          .textInputAutocapitalization(.never)
          .keyboardType(.emailAddress)
          .accessibilityIdentifier("radroots.settings.profile.nip05")
        Toggle("Automated account", isOn: $settingsStore.profileBot)
        Button("Save profile update") { Task { await settingsStore.saveProfile() } }
          .disabled(settingsStore.isWorking)
          .accessibilityIdentifier("radroots.settings.profile.save")
        if let status = settingsStore.profileStatus {
          LabeledContent("Publication", value: status.honestSummary)
          if status.state.canAdvance {
            Button("Retry profile publication") {
              Task { await settingsStore.advanceProfile() }
            }
          }
          if status.state.canCancel {
            Button("Cancel profile publication", role: .destructive) {
              Task { await settingsStore.cancelProfile() }
            }
          }
        }
      }
      Section("Network environment") {
        Picker("Environment", selection: $settingsStore.networkEnvironment) {
          ForEach(TeraSettingsNetworkEnvironment.allCases) { environment in
            Text(display(environment.rawValue)).tag(environment)
          }
        }
        .accessibilityIdentifier("radroots.settings.network.environment")
      }
      Section("Nostr relay preferences") {
        ForEach($settingsStore.relays) { $relay in
          VStack(alignment: .leading) {
            TextField("wss://relay.example", text: $relay.url)
              .textInputAutocapitalization(.never)
              .keyboardType(.URL)
            Picker("Access", selection: $relay.access) {
              ForEach(TeraRelayAccessPreference.allCases) { access in
                Text(access.label).tag(access)
              }
            }
          }
        }
        .onDelete(perform: settingsStore.removeRelays)
        Button("Add relay") { settingsStore.addRelay() }
          .accessibilityIdentifier("radroots.settings.relays.add")
      }
      Section("Live relay status") {
        if snapshot.relay?.relays.isEmpty != false {
          Text("No relay is configured for this profile.")
            .foregroundStyle(.secondary)
        }
        ForEach(snapshot.relay?.relays ?? [], id: \.url) { relay in
          VStack(alignment: .leading, spacing: 4) {
            Text(relay.url).font(.caption.monospaced())
            Text("\(relay.access.label) · read \(relay.readState) · write \(relay.writeState)")
              .font(.caption2)
              .foregroundStyle(.secondary)
          }
        }
        Button("Retry local network") { Task { await todayStore.reload() } }
          .accessibilityIdentifier("radroots.settings.retry.network")
      }
      Section("Blossom preferences") {
        Picker("Trust", selection: $settingsStore.blossomAuthority) {
          ForEach(TeraBlossomAuthorityPreference.allCases) { authority in
            Text(display(authority.rawValue)).tag(authority)
          }
        }
        TextField("Primary HTTPS origin", text: $settingsStore.blossomPrimaryOrigin)
          .textInputAutocapitalization(.never)
          .keyboardType(.URL)
          .accessibilityIdentifier("radroots.settings.blossom.primary")
        TextField(
          "Fallback HTTPS origins, one per line",
          text: $settingsStore.blossomFallbackOrigins,
          axis: .vertical
        )
        .lineLimit(2 ... 5)
        .textInputAutocapitalization(.never)
        .keyboardType(.URL)
        Toggle("Cellular downloads", isOn: $settingsStore.allowCellularDownloads)
        Toggle("Cellular uploads", isOn: $settingsStore.allowCellularUploads)
        Toggle("Background transfers", isOn: $settingsStore.allowBackgroundTransfers)
      }
      TeraStorageSettingsSection(settingsStore: settingsStore, todayStore: todayStore)
      Section("Blossom service status") {
        if let configuration = addStore.blossomConfiguration {
          LabeledContent("Origin", value: configuration.primaryOrigin)
            .accessibilityIdentifier("radroots.settings.blossom.origin")
          LabeledContent("Configuration", value: abbreviated(configuration.configFingerprint))
            .accessibilityIdentifier("radroots.settings.blossom.fingerprint")
        } else {
          Text("No photo service is configured for this network profile.")
            .foregroundStyle(.secondary)
        }
        if let evidence = addStore.blossomEvidence {
          LabeledContent("Service state", value: display(evidence.state))
            .accessibilityIdentifier("radroots.settings.blossom.state")
          LabeledContent("Connection", value: display(evidence.transportSecurity))
          if evidence.lastSuccessfulState != "none" {
            LabeledContent("Last success", value: display(evidence.lastSuccessfulState))
          }
          if let status = evidence.httpStatus {
            LabeledContent("HTTP status", value: String(status))
              .accessibilityIdentifier("radroots.settings.blossom.http_status")
          }
          if let errorCode = evidence.errorCode {
            LabeledContent("Last error", value: display(errorCode))
              .accessibilityIdentifier("radroots.settings.blossom.error_code")
          }
          if let serverErrorCode = evidence.serverErrorCode {
            LabeledContent("Server error", value: display(serverErrorCode))
              .accessibilityIdentifier("radroots.settings.blossom.server_error_code")
          }
          if evidence.attempts > 0 {
            LabeledContent("Attempts", value: String(evidence.attempts))
          }
          if evidence.possibleOrphan {
            Text("The server may contain an upload whose verification did not complete.")
              .foregroundStyle(.orange)
          }
        }
        LabeledContent(
          "Photo library",
          value: addStore.mediaSupport.library ? "Ready" : "Unavailable"
        )
        LabeledContent("Camera", value: addStore.mediaSupport.camera ? "Ready" : "Unavailable")
        Button {
          Task { await addStore.checkPhotoService() }
        } label: {
          if addStore.isCheckingBlossom {
            ProgressView()
          } else {
            Text("Check photo service")
          }
        }
        .disabled(addStore.isCheckingBlossom || addStore.blossomConfiguration == nil)
        .accessibilityIdentifier("radroots.settings.retry.blossom")
      }
      Section("Runtime") {
        LabeledContent("Crate", value: snapshot.crateName)
        LabeledContent("Version", value: snapshot.crateVersion)
        LabeledContent("State", value: snapshot.isClosed ? "Closed" : "Running")
      }
      Section {
        if let message = diagnosticsStore.message {
          Text(message)
            .foregroundStyle(.secondary)
        }
        Button("Prepare diagnostics export") {
          Task { await diagnosticsStore.prepare(snapshot: snapshot) }
        }
        .disabled(diagnosticsStore.isPreparing)
        .accessibilityIdentifier("radroots.settings.diagnostics")
      } header: {
        Text("Privacy-safe diagnostics")
      } footer: {
        Text(
          "Exports contain bounded lifecycle codes and runtime status only. Posts, keys, credentials, endpoint URLs, and local paths are excluded."
        )
      }
    }
    .navigationTitle("Settings")
    .task { await settingsStore.load(profile: meStore.snapshot?.profile) }
    .radrootsDocumentExporter(preparedExport: $diagnosticsStore.preparedExport) { result in
      diagnosticsStore.completeExport(result)
    }
    .accessibilityIdentifier("radroots.support.settings.view")
  }

  private var abbreviatedPublicKey: String {
    let key = snapshot.identity.publicKeyHex
    guard key.count > 16 else { return key }
    return "\(key.prefix(8))…\(key.suffix(8))"
  }

  private func abbreviated(_ value: String) -> String {
    guard value.count > 16 else { return value }
    return "\(value.prefix(8))…\(value.suffix(8))"
  }

  private func display(_ value: String) -> String {
    value.replacingOccurrences(of: "_", with: " ")
  }
}
