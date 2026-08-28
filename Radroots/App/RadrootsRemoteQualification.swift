import Foundation
import RadrootsKit
import RadrootsKitBindings

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
  let runtimeMode: String

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
      let runtimeMode: String
      switch environment[networkProfileKey] {
      case "public": runtimeMode = "production"
      case "simulator": runtimeMode = "simulator"
      default:
        throw RadrootsConfigurationError.invalid("qualification_network_profile")
      }
      guard !relays.isEmpty, blossoms.count == 1 else {
        throw RadrootsConfigurationError.invalid("qualification_blossom_origin")
      }
      if runtimeMode == "simulator" {
        guard
          relays.allSatisfy({ isLoopbackEndpoint($0, schemes: ["ws"]) }),
          blossoms.allSatisfy({ isLoopbackEndpoint($0, schemes: ["http"]) })
        else {
          throw RadrootsConfigurationError.invalid("qualification_loopback_endpoint")
        }
      }
      let mediaFile = try environment[mediaRelativePathKey].map { raw in
        guard raw == "qualification/input.png" else {
          throw RadrootsConfigurationError.invalid("qualification_media_file")
        }
        return RadrootsFileReference(scope: .data, relativePath: raw)
      }
      return Self(
        runID: runID,
        relayURLs: relays,
        blossomOrigins: blossoms,
        mediaFile: mediaFile,
        runtimeMode: runtimeMode
      )
    }

    private static func requiredRunID(_ raw: String?) throws -> String {
      guard let raw,
        (8...64).contains(raw.utf8.count),
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

    private static func isLoopbackEndpoint(_ raw: String, schemes: Set<String>) -> Bool {
      guard
        let components = URLComponents(string: raw),
        let scheme = components.scheme?.lowercased(),
        schemes.contains(scheme),
        components.host == "127.0.0.1",
        components.port != nil,
        components.user == nil,
        components.password == nil,
        components.query == nil,
        components.fragment == nil,
        components.path.isEmpty || components.path == "/"
      else {
        return false
      }
      return true
    }
  #else
    static func current(environment _: [String: String] = [:]) throws -> Self? {
      nil
    }
  #endif
}

#if DEBUG
  private struct RadrootsRemoteQualificationBlossomAuthorization: Codable {
    let schema: String
    let schemaVersion: UInt16
    let id: String
    let pubkey: String
    let createdAt: UInt64
    let kind: UInt32
    let tags: [[String]]
    let content: String
    let signature: String

    enum CodingKeys: String, CodingKey {
      case schema
      case schemaVersion = "schema_version"
      case id
      case pubkey
      case createdAt = "created_at"
      case kind
      case tags
      case content
      case signature = "sig"
    }
  }

  enum RadrootsRemoteQualificationEvidence {
    static func recordBlossomAuthorization(
      request: HostSigningRequest,
      signatureHex: String
    ) throws {
      guard try RadrootsRemoteQualificationEnvironment.current() != nil else { return }
      let evidence = RadrootsRemoteQualificationBlossomAuthorization(
        schema: "radroots-ios-remote-qualification-bud11-v1",
        schemaVersion: 1,
        id: request.expectedEventId,
        pubkey: request.publicKey,
        createdAt: request.createdAtUnixS,
        kind: request.kind,
        tags: request.tags,
        content: request.content,
        signature: signatureHex
      )
      let encoder = JSONEncoder()
      encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
      let data = try encoder.encode(evidence)
      guard
        let documents = FileManager.default.urls(
          for: .documentDirectory,
          in: .userDomainMask
        ).first
      else {
        throw CocoaError(.fileNoSuchFile)
      }
      try data.write(
        to: documents.appendingPathComponent(
          "radroots-remote-qualification-bud11.json"
        ),
        options: .atomic
      )
    }
  }

  final class RadrootsRemoteQualificationUserPresence: RadrootsUserPresence, Sendable {
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
