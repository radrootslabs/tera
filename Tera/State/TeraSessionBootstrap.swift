import Foundation
import UIKit

extension TeraSessionStore {
  @MainActor
  static func production(
    bundle: Bundle = .main,
    runtimeClient: TeraRuntimeClient = .production()
  ) throws -> TeraSessionStore {
    guard let bundleIdentifier = bundle.bundleIdentifier else {
      throw TeraConfigurationError.missing("bundle_identifier")
    }
    let qualification = try TeraRemoteQualificationEnvironment.current()
    #if DEBUG
      let qualificationEvidenceStore = try TeraRemoteQualificationEvidence.prepare()
    #else
      let qualificationEvidenceStore: TeraRemoteQualificationEvidenceStore? = nil
    #endif
    let servicePrefix =
      try qualification?.keychainServicePrefix
      ?? requiredString(
        "TERA_IOS_KEYCHAIN_SERVICE_PREFIX", bundle: bundle
      )
    let bootstrap = try TeraConfigurationBootstrap(
      runtimeMode: qualification?.runtimeMode
        ?? requiredString("TERA_IOS_RUNTIME_MODE", bundle: bundle),
      relayURLs: qualification?.relayURLs
        ?? array("TERA_IOS_NOSTR_RELAY_URLS", bundle: bundle),
      blossomOrigins: qualification?.blossomOrigins
        ?? array("TERA_IOS_BLOSSOM_ORIGINS", bundle: bundle),
      keychainServicePrefix: servicePrefix,
      bundleIdentifier: bundleIdentifier,
      appMetadata: metadata(bundleIdentifier: bundleIdentifier, bundle: bundle)
    )
    let roots = try TeraRemoteQualificationEnvironment.applicationFileRoots(
      appIdentifier: bundleIdentifier
    )
    let protectedData = TeraProtectedDataMonitor(
      available: UIApplication.shared.isProtectedDataAvailable
    )
    return try TeraSessionStore(
      configurationStore: TeraConfigurationStore(bootstrap: bootstrap, roots: roots),
      identityStore: .production(
        servicePrefix: servicePrefix,
        protectedDataAvailable: { protectedData.isAvailable() },
        qualification: qualification
      ),
      runtimeClient: runtimeClient,
      roots: roots,
      protectedData: protectedData,
      automatesQualificationIdentity: qualification?.automatesIdentity == true,
      qualificationEvidenceStore: qualificationEvidenceStore
    )
  }

  @MainActor
  private static func metadata(bundleIdentifier: String, bundle: Bundle) -> TeraRuntimeAppMetadata {
    TeraRuntimeAppMetadata(
      bundleIdentifier: bundleIdentifier,
      version: bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
          ?? "0",
      buildNumber: bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "0",
      buildSHA: normalizedOptional(
          bundle.object(forInfoDictionaryKey: "GIT_SHA") as? String
      )
    )
  }

  @MainActor
  private static func requiredString(_ key: String, bundle: Bundle) throws -> String {
    guard let value = normalizedOptional(bundle.object(forInfoDictionaryKey: key) as? String) else {
      throw TeraConfigurationError.missing(key)
    }
    return value
  }

  @MainActor
  private static func array(_ key: String, bundle: Bundle) -> [String] {
    if let values = bundle.object(forInfoDictionaryKey: key) as? [String] {
      return values.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
    }
    guard let raw = normalizedOptional(bundle.object(forInfoDictionaryKey: key) as? String) else {
      return []
    }
    return raw.components(separatedBy: CharacterSet(charactersIn: ",; \n\r\t"))
      .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
      .filter { !$0.isEmpty }
  }

  @MainActor
  private static func normalizedOptional(_ value: String?) -> String? {
    guard let value else { return nil }
    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty || trimmed == "unknown" ? nil : trimmed
  }
}
