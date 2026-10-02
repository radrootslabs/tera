import CryptoKit
import Foundation
import RadrootsKit
@testable import TeraApp
import TeraKitBindings
import XCTest

@MainActor
struct TeraHistoricalOwnerFixture {
  let root: URL
  let host: [String: Any]
  let rust: [String: Any]
  let native: [String: Any]

  init() throws {
    let source = try XCTUnwrap(Bundle(for: TeraHistoricalOwnerTests.self).url(forResource: "legacy_upgrade_v1", withExtension: nil))
    let manifest = try Self.object(source.appendingPathComponent("manifest.json"))
    XCTAssertEqual(manifest["state"] as? String, "HISTORICAL_WRITER_ADMITTED_CURRENT_READERS_PENDING")
    root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    for (relative, hash) in try XCTUnwrap(manifest["files"] as? [String: String]) {
      let components = relative.split(separator: "/", omittingEmptySubsequences: false)
      guard !relative.hasPrefix("/"), !components.isEmpty,
        components.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." })
      else {
        throw CocoaError(.fileReadInvalidFileName)
      }
      var input = source
      for component in components {
        input.appendPathComponent(String(component))
        guard try input.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink == false else {
          throw CocoaError(.fileReadInvalidFileName)
        }
      }
      let output = root.appendingPathComponent(relative)
      let bytes = try Data(contentsOf: input)
      XCTAssertEqual(Self.hash(bytes), hash)
      try FileManager.default.createDirectory(at: output.deletingLastPathComponent(), withIntermediateDirectories: true)
      try bytes.write(to: output, options: [.atomic])
    }
    for name in ["data", "cache", "tmp", "logs"] {
      try FileManager.default.createDirectory(at: root.appendingPathComponent(name), withIntermediateDirectories: true)
    }
    host = try Self.object(root.appendingPathComponent("host-metadata.json"))
    rust = try Self.object(root.appendingPathComponent("rust-metadata.json"))
    native = try Self.object(root.appendingPathComponent("native-metadata.json"))
  }

  func text(_ object: [String: Any], _ key: String) throws -> String {
    try XCTUnwrap(object[key] as? String)
  }

  func number(_ object: [String: Any], _ key: String) throws -> UInt64 {
    try XCTUnwrap(object[key] as? NSNumber).uint64Value
  }

  var publicKey: String {
    get throws { try text(rust, "public_key") }
  }

  var generation: String {
    get throws { try text(host, "source_generation") }
  }

  func roots() throws -> RadrootsAppleFileRoots {
    let base = try RadrootsAppleFileRoots(appIdentifier: "dev.local.radroots",
                                          dataRoot: root.appendingPathComponent("data"), cacheRoot: root.appendingPathComponent("cache"),
                                          temporaryRoot: root.appendingPathComponent("tmp"), logsRoot: root.appendingPathComponent("logs"))
    return try TeraDurableMediaRoots.selectingStaging(in: base)
  }

  func blob() throws -> RadrootsStagedBlobReference {
    let hash = try text(host, "media_sha256")
    let count = try XCTUnwrap(Int(exactly: number(host, "media_bytes")))
    return try RadrootsStagedBlobReference(blobID: hash, sizeBytes: count, mediaType: "image/png", filenameHint: hash + ".png")
  }

  func bootstrap() throws -> TeraConfigurationBootstrap {
    try TeraConfigurationBootstrap(runtimeMode: "production", relayURLs: ["wss://relay-one.example", "wss://relay-two.example"],
                                   blossomOrigins: ["https://blossom.example"], keychainServicePrefix: text(host, "keychain_service_prefix"),
                                   bundleIdentifier: text(host, "bundle_identifier"),
                                   appMetadata: TeraRuntimeAppMetadata(bundleIdentifier: text(host, "bundle_identifier"), version: "1.0.0-alpha", buildNumber: "1", buildSHA: nil))
  }

  func runtime() async throws -> TeraRuntime {
    try await TeraRuntime(applicationSupportDirectory: roots().dataRoot.path, publicKeyHex: publicKey,
                          sourceGenerationHex: generation, sourceGenerationCreatedAtUnixMs: number(host, "generation_created_at_unix_ms"), protectedData: .available)
  }

  func assertStatuses(_ runtime: TeraRuntime) async throws {
    let statuses = try XCTUnwrap(rust["drafts"] as? [[String: Any]])
    XCTAssertEqual(statuses.count, 106)
    for expected in statuses {
      let actual = try await runtime.phase1DraftStatus(draftId: text(expected, "draft_id"))
      XCTAssertEqual(actual.authorPublicKey, try publicKey)
      XCTAssertEqual(actual.revision, try number(expected, "revision"))
      XCTAssertEqual(actual.cardId, try text(expected, "card_id"))
      XCTAssertEqual(actual.operationId, expected["operation_id"] as? String)
      XCTAssertEqual(actual.createdAtUnixMs, try number(expected, "created_at_unix_ms"))
      XCTAssertEqual(actual.updatedAtUnixMs, try number(expected, "updated_at_unix_ms"))
      XCTAssertEqual(actual.state, try XCTUnwrap(Self.states[text(expected, "state")]))
      if let settled = expected["settlement"] as? [String: Any] {
        let actual = try XCTUnwrap(actual.settlement)
        XCTAssertEqual(UInt64(actual.signed), try number(settled, "signed"))
        XCTAssertEqual(UInt64(actual.admitted), try number(settled, "admitted"))
        XCTAssertEqual(UInt64(actual.pending), try number(settled, "pending"))
        XCTAssertEqual(UInt64(actual.deliveryPlans), try number(settled, "delivery_plans"))
        XCTAssertEqual(UInt64(actual.deliverySatisfied), try number(settled, "delivery_satisfied"))
      } else {
        XCTAssertNil(actual.settlement)
      }
    }
  }

  private static let states: [String: FfiOutboxState] = [
    "Draft": .draft, "MediaPreparing": .mediaPreparing, "MediaUploading": .mediaUploading,
    "ReadyToSign": .readyToSign, "Signing": .signing, "Signed": .signed, "Queued": .queued,
    "Delivering": .delivering, "PartiallyDelivered": .partiallyDelivered, "Retryable": .retryable,
    "Terminal": .terminal, "Cancelled": .cancelled, "Complete": .complete,
  ]

  static func object(_ path: URL) throws -> [String: Any] {
    try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: path)) as? [String: Any])
  }

  static func hash(_ bytes: Data) -> String {
    SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
  }

  func remove() {
    try? FileManager.default.removeItem(at: root)
  }
}
