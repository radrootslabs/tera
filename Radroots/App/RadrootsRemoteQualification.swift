import Foundation
import RadrootsKit
import RadrootsKitBindings

enum RadrootsQualificationNetworkMode: Sendable, Equatable {
  case isolatedLoopback
  case publicEndpoint

  init(profile: String?) throws {
    switch profile {
    case "simulator": self = .isolatedLoopback
    case "public": self = .publicEndpoint
    default:
      throw RadrootsConfigurationError.invalid("qualification_network_profile")
    }
  }

  var runtimeMode: String {
    switch self {
    case .isolatedLoopback: "simulator"
    case .publicEndpoint: "production"
    }
  }

  var permitsAutomatedUserPresence: Bool {
    self == .isolatedLoopback
  }

  var permitsTestSecretPolicy: Bool {
    self == .isolatedLoopback
  }
}

struct RadrootsQualificationEndpoint: Sendable, Equatable {
  enum Role: Sendable {
    case relay
    case blossom
  }

  let rawValue: String

  init(_ raw: String, role: Role, mode: RadrootsQualificationNetworkMode) throws {
    guard let value = URLComponents(string: raw),
      let scheme = value.scheme?.lowercased(),
      let host = value.host?.lowercased(),
      !host.isEmpty,
      value.user == nil,
      value.password == nil,
      value.query == nil,
      value.fragment == nil
    else {
      throw RadrootsConfigurationError.invalid("qualification_endpoint")
    }
    let expectedScheme = switch (role, mode) {
    case (.relay, .isolatedLoopback): "ws"
    case (.blossom, .isolatedLoopback): "http"
    case (.relay, .publicEndpoint): "wss"
    case (.blossom, .publicEndpoint): "https"
    }
    let loopback = host == "127.0.0.1"
    let validHost = switch mode {
    case .isolatedLoopback:
      loopback
        && value.port.map { 1 ... 65535 ~= $0 } == true
        && (value.path.isEmpty || value.path == "/")
    case .publicEndpoint: !loopback && host != "::1" && host != "localhost"
    }
    guard scheme == expectedScheme, validHost else {
      throw RadrootsConfigurationError.invalid("qualification_endpoint_policy")
    }
    rawValue = raw
  }
}

struct RadrootsRemoteQualificationEnvironment: Sendable, Equatable {
  static let enabledKey = "RADROOTS_IOS_UI_TEST_REMOTE"
  static let runIDKey = "RADROOTS_IOS_UI_TEST_RUN_ID"
  static let relayURLsKey = "RADROOTS_IOS_UI_TEST_NOSTR_RELAY_URLS"
  static let blossomOriginsKey = "RADROOTS_IOS_UI_TEST_BLOSSOM_ORIGINS"
  static let mediaRelativePathKey = "RADROOTS_IOS_UI_TEST_MEDIA_RELATIVE_PATH"
  static let networkProfileKey = "RADROOTS_IOS_UI_TEST_NETWORK_PROFILE"

  let runID: String
  let relayURLs: [String]
  let blossomOrigins: [String]
  let mediaFile: RadrootsFileReference?
  let networkMode: RadrootsQualificationNetworkMode

  var runtimeMode: String {
    networkMode.runtimeMode
  }

  var automatesIdentity: Bool {
    networkMode.permitsAutomatedUserPresence && networkMode.permitsTestSecretPolicy
  }

  private static let mediaFixtureBase64 =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="

  var keychainServicePrefix: String {
    "org.radroots.ios.remote-qualification.\(runID)"
  }

  var identityMetadataKeyPrefix: String {
    "\(keychainServicePrefix).identity"
  }

  var backgroundTransferIdentifierSuffix: String {
    "remote-qualification.\(runID)"
  }

  func isolatedFileRoots(from base: RadrootsAppleFileRoots) throws
    -> RadrootsAppleFileRoots
  {
    try RadrootsAppleFileRoots(
      appIdentifier: base.appIdentifier,
      dataRoot: base.dataRoot.appendingPathComponent(runID, isDirectory: true),
      cacheRoot: base.cacheRoot.appendingPathComponent(runID, isDirectory: true),
      temporaryRoot: base.temporaryRoot.appendingPathComponent(runID, isDirectory: true)
    )
  }

  static func applicationFileRoots(appIdentifier: String) throws -> RadrootsAppleFileRoots {
    let base = try RadrootsAppleFileRoots.appContainer(appIdentifier: appIdentifier)
    #if DEBUG
      return try current()?.isolatedFileRoots(from: base) ?? base
    #else
      return base
    #endif
  }

  static func backgroundTransferIdentifier(appIdentifier: String) throws -> String {
    #if DEBUG
      if let qualification = try current() {
        return "\(appIdentifier.lowercased()).\(qualification.backgroundTransferIdentifierSuffix)"
      }
    #endif
    return "\(appIdentifier.lowercased()).background.transfer"
  }

  static func mediaFixtureData() throws -> Data {
    guard let data = Data(base64Encoded: mediaFixtureBase64), !data.isEmpty else {
      throw RadrootsConfigurationError.invalid("qualification_media_fixture")
    }
    return data
  }

  #if DEBUG
    static func current(
      environment: [String: String] = ProcessInfo.processInfo.environment
    ) throws -> Self? {
      guard environment[enabledKey] == "1" else { return nil }
      let runID = try requiredRunID(environment[runIDKey])
      let relays = separatedValues(environment[relayURLsKey])
      let blossoms = separatedValues(environment[blossomOriginsKey])
      let networkMode = try RadrootsQualificationNetworkMode(
        profile: environment[networkProfileKey]
      )
      guard !relays.isEmpty, blossoms.count == 1 else {
        throw RadrootsConfigurationError.invalid("qualification_blossom_origin")
      }
      let relayEndpoints = try relays.map {
        try RadrootsQualificationEndpoint($0, role: .relay, mode: networkMode)
      }
      let blossomEndpoints = try blossoms.map {
        try RadrootsQualificationEndpoint($0, role: .blossom, mode: networkMode)
      }
      guard Set(relayEndpoints.map(\.rawValue)).count == relayEndpoints.count else {
        throw RadrootsConfigurationError.invalid("qualification_endpoint_duplicate")
      }
      let mediaFile = try environment[mediaRelativePathKey].map { raw in
        guard raw == "qualification/input.png" else {
          throw RadrootsConfigurationError.invalid("qualification_media_file")
        }
        return RadrootsFileReference(scope: .data, relativePath: raw)
      }
      return Self(
        runID: runID,
        relayURLs: relayEndpoints.map(\.rawValue),
        blossomOrigins: blossomEndpoints.map(\.rawValue),
        mediaFile: mediaFile,
        networkMode: networkMode
      )
    }

    private static func requiredRunID(_ raw: String?) throws -> String {
      guard let raw,
        (8 ... 64).contains(raw.utf8.count),
        raw == raw.lowercased(),
        raw.unicodeScalars.allSatisfy({
          CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyz0123456789-")
            .contains($0)
        }),
        raw.first != "-",
        raw.last != "-"
      else {
        throw RadrootsConfigurationError.invalid("qualification_run_id")
      }
      return raw
    }

    private static func separatedValues(_ raw: String?) -> [String] {
      guard let raw else { return [] }
      return raw.components(separatedBy: CharacterSet(charactersIn: ",; \n\r\t"))
        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
    }

  #else
    static func current(environment _: [String: String] = [:]) throws -> Self? {
      nil
    }
  #endif
}

#if DEBUG
  final class RadrootsRemoteQualificationUserPresence: RadrootsUserPresence, Sendable {
    init(mode: RadrootsQualificationNetworkMode) throws {
      guard mode.permitsAutomatedUserPresence, mode.permitsTestSecretPolicy else {
        throw RadrootsConfigurationError.invalid("qualification_user_presence")
      }
    }

    func currentStatus() async throws -> RadrootsUserPresenceStatus {
      RadrootsUserPresenceStatus(
        support: .deviceCredential,
        biometryKind: .none,
        canEvaluateDeviceCredential: true,
        canEvaluateBiometrics: false
      )
    }

    func verify(
      _ request: RadrootsUserPresenceRequest
    ) async throws -> RadrootsUserPresenceResult {
      RadrootsUserPresenceResult(policy: request.policy, verified: true)
    }
  }
#endif
