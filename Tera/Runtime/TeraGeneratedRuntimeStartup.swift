import Foundation
import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  static func start(
    configuration: TeraRuntimeLaunchConfiguration, localBackups: Bool = false
  ) async throws -> TeraRuntimeBackendStart {
    var createdRuntime: TeraRuntime?
    do {
      let mediaUse = try TeraMediaProcessUse.admit(applicationSupportDirectory: configuration.applicationSupportDirectory)
      defer { withExtendedLifetime(mediaUse) {} }
      let constructor = localBackups ? TeraRuntime.withHostSignerAndLocalBackups : TeraRuntime.withHostSigner
      let runtime = try await constructor(
        configuration.applicationSupportDirectory,
        configuration.publicKeyHex,
        configuration.sourceGenerationHex,
        configuration.sourceGenerationCreatedAtUnixMilliseconds,
        configuration.protectedData.generatedValue,
        TeraGeneratedHostSigner(signer: configuration.signer)
      )
      createdRuntime = runtime
      runtime.setAppInfoPlatform(
        platform: "iOS",
        bundleId: configuration.app.bundleIdentifier,
        version: configuration.app.version,
        buildNumber: configuration.app.buildNumber,
        buildSha: configuration.app.buildSHA
      )

      let backend = TeraGeneratedRuntimeBackend(runtime: runtime)
      let currentSettings = try await backend.mobileSettings()
      if configuration.adoptBootstrapSettings
        || currentSettings.revision == 1 && currentSettings.identity.identities.isEmpty
      {
        _ = try await backend.replaceMobileSettings(
          input: configuration.bootstrapSettingsReplacement(current: currentSettings)
        )
      }
      _ = try await runtime.phase1ApplySettingsToRuntime()
      return try await TeraRuntimeBackendStart(
        backend: backend,
        snapshot: backend.snapshot()
      )
    } catch {
      if let createdRuntime {
        _ = try? await createdRuntime.shutdown()
      }
      throw TeraGeneratedRuntimeFailure.from(error)
    }
  }
}

extension TeraRuntimeLaunchConfiguration {
  fileprivate func bootstrapSettingsReplacement(
    current: TeraMobileSettings
  ) -> TeraReplaceSettings {
    let environment = networkProfile.settingsValue
    var relays = writableRelays.map {
      TeraRelayPreference(url: $0, access: .readWrite)
    }
    if networkProfile != .simulator,
       !relays.contains(where: { $0.url == TeraNetworkValidator.canonicalRelay })
    {
      relays.insert(
        TeraRelayPreference(
          url: TeraNetworkValidator.canonicalRelay,
          access: .readWrite
        ),
        at: 0
      )
    }
    let authority = blossom?.endpointAuthority.settingsValue
      ?? current.blossomAuthority
    let primaryOrigin = blossom?.primaryOrigin ?? current.blossomPrimaryOrigin
    let fallbackOrigins = blossom?.fallbackOrigins ?? current.blossomFallbackOrigins
    return TeraReplaceSettings(
      expectedRevision: current.revision,
      networkEnvironment: environment,
      relays: relays,
      blossomAuthority: authority,
      blossomPrimaryOrigin: primaryOrigin,
      blossomFallbackOrigins: fallbackOrigins,
      allowCellularDownloads: current.allowCellularDownloads,
      allowCellularUploads: current.allowCellularUploads,
      allowBackgroundTransfers: current.allowBackgroundTransfers,
      mediaCacheBytes: current.mediaCacheBytes,
      mediaCacheArtifacts: current.mediaCacheArtifacts
    )
  }
}

extension TeraRuntimeNetworkProfile {
  fileprivate var settingsValue: TeraSettingsNetworkEnvironment {
    switch self {
    case .publicNetwork: .publicNetwork
    case .simulator: .simulator
    case .device: .physicalDevice
    }
  }
}

extension TeraBlossomEndpointAuthority {
  fileprivate var settingsValue: TeraBlossomAuthorityPreference {
    switch self {
    case .publicWebPKI: .publicWebPKI
    case .loopbackDevelopment: .loopbackDevelopment
    case .privateNetworkDevelopment: .privateNetworkDevelopment
    }
  }
}

extension TeraRuntimeClient {
  static func production() -> TeraRuntimeClient {
    TeraRuntimeClient { configuration in
      try await TeraGeneratedRuntimeBackend.start(configuration: configuration)
    }
  }
}

extension TeraProtectedDataState {
  fileprivate var generatedValue: ProtectedDataAvailability {
    switch self {
    case .available: .available
    case .unavailable: .unavailable
    }
  }
}
