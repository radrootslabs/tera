import CoreGraphics
import Foundation
import ImageIO
import RadrootsKit
import UniformTypeIdentifiers
import XCTest

@MainActor
final class TeraHistoricalFixtureWriterTests: XCTestCase {
  func testProduceHistoricalOwnerBytes() async throws {
    let environment = ProcessInfo.processInfo.environment
    let output = try URL(fileURLWithPath: XCTUnwrap(environment["TERA_LEGACY_FIXTURE_OUTPUT"]), isDirectory: true)
    let roots = try roots(output)
    if environment["TERA_LEGACY_FIXTURE_PHASE"] == "host" {
      try await produceHost(output, roots: roots)
    } else {
      XCTAssertEqual(environment["TERA_LEGACY_FIXTURE_PHASE"], "transfer")
      try await produceTransfer(output, roots: roots)
    }
  }

  private func roots(_ output: URL) throws -> RadrootsAppleFileRoots {
    for directory in ["data", "cache", "tmp", "logs"] {
      try FileManager.default.createDirectory(at: output.appendingPathComponent(directory), withIntermediateDirectories: true)
    }
    return try RadrootsAppleFileRoots(appIdentifier: "dev.local.radroots",
                                      dataRoot: output.appendingPathComponent("data"), cacheRoot: output.appendingPathComponent("cache"),
                                      temporaryRoot: output.appendingPathComponent("tmp"), logsRoot: output.appendingPathComponent("logs"))
  }

  private func produceHost(_ output: URL, roots: RadrootsAppleFileRoots) async throws {
    let metadata = RadrootsRuntimeAppMetadata(bundleIdentifier: "dev.local.radroots", version: "1.0.0-alpha", buildNumber: "1", buildSHA: nil)
    let bootstrap = RadrootsConfigurationBootstrap(runtimeMode: "production",
                                                   relayURLs: ["wss://relay-one.example", "wss://relay-two.example"], blossomOrigins: ["https://blossom.example"],
                                                   keychainServicePrefix: "org.radroots.field_ios.local", bundleIdentifier: "dev.local.radroots", appMetadata: metadata)
    let configuration = RadrootsConfigurationStore(bootstrap: bootstrap, roots: roots, clock: .fixed(unixSeconds: 1_700_000_000))
    let selected = try await configuration.load()
    XCTAssertEqual(selected.generation, 1)
    let generation = try await configuration.sourceGeneration()
    let source = try RadrootsFileReference(scope: .temporary, relativePath: "synthetic_source.png")
    try RadrootsAppleFileAccess(roots: roots).write(.inline(syntheticPNG()), to: source)
    let image = try await RadrootsAppleMediaPreparer(roots: roots).prepareImage(RadrootsAppleImagePreparationRequest(source: .file(source)))
    let staged = try roots.stagedBlobURL(for: image.file)
    let value: [String: Any] = ["source_generation": generation.generationHex,
                                "generation_created_at_unix_ms": generation.createdAtUnixMilliseconds,
                                "media_sha256": image.sha256, "media_bytes": image.file.sizeBytes,
                                "media_width": image.width, "media_height": image.height,
                                "staged_relative_path": String(staged.path.dropFirst(output.path.count + 1)),
                                "bundle_identifier": selected.bundleIdentifier, "keychain_service_prefix": selected.keychainServicePrefix]
    try write(value, to: output.appendingPathComponent("host-metadata.json"))
    // Keep the actual historical sanitized blob, never the temporary source.
    try RadrootsAppleFileAccess(roots: roots).delete(source)
  }

  private func produceTransfer(_ output: URL, roots: RadrootsAppleFileRoots) async throws {
    let host = try dictionary(output.appendingPathComponent("host-metadata.json"))
    let rust = try dictionary(output.appendingPathComponent("rust-metadata.json"))
    let job = try XCTUnwrap(rust["native_pending"] as? [String: Any])
    let hash = try XCTUnwrap(job["sha256"] as? String)
    let count = try XCTUnwrap(Int(exactly: XCTUnwrap(job["byte_size"] as? NSNumber).uint64Value))
    let blob = try RadrootsStagedBlobReference(blobID: hash, sizeBytes: count, mediaType: "image/png", filenameHint: hash + ".png")
    let id = try "radroots.add.\(XCTUnwrap(job["draft_id"] as? String)).\(XCTUnwrap(job["revision"] as? NSNumber)).\(XCTUnwrap(job["operation_id"] as? String))"
    let request = try RadrootsBackgroundTransferRequest(identifier: RadrootsBackgroundTransferIdentifier(id),
                                                        remoteURL: XCTUnwrap(try URL(string: XCTUnwrap(job["remote_url"] as? String))), method: .put,
                                                        operation: .upload(source: .stagedBlob(blob)), responsePolicy: .boundedJSON(), expectedSourceSHA256: hash)
    let descriptor: [String: Any] = ["url": "https://blossom.example/\(hash).png", "sha256": hash, "size": count, "type": "image/png", "uploaded": 1_786_000_000]
    let body = try JSONSerialization.data(withJSONObject: descriptor, options: [.sortedKeys])
    let response = try RadrootsBackgroundTransferResponse(statusCode: 200, mediaType: "application/json", body: body)
    let snapshot = try RadrootsBackgroundTransferSnapshot(request: request, state: .awaitingVerification,
                                                          progress: RadrootsBackgroundTransferProgress(bytesTransferred: Int64(count), totalBytesExpected: Int64(count)),
                                                          response: response, updatedAt: Date())
    let store = RadrootsAppleBackgroundTransferStore(roots: roots)
    try await store.saveSnapshot(snapshot)
    let loaded = try await store.loadSnapshots()
    XCTAssertEqual(loaded.count, 1)
    XCTAssertEqual(loaded[0].request.headers, [:])
    XCTAssertEqual(loaded[0].identifier.rawValue, id)
    try write(["native_identifier": id, "media_sha256": hash, "public_key": XCTUnwrap(rust["public_key"] as? String),
               "source_generation": XCTUnwrap(host["source_generation"] as? String),
               "state": "awaitingVerification", "limit": "Actual old typed persistence; no physical OS upload/termination claim."], to: output.appendingPathComponent("native-metadata.json"))
  }

  private func dictionary(_ url: URL) throws -> [String: Any] {
    try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
  }

  private func write(_ value: [String: Any], to url: URL) throws {
    try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys]).write(to: url, options: [.atomic])
  }

  private func syntheticPNG() throws -> Data {
    let bytes = Data([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 255, 0, 255, 255, 0, 255, 255, 255])
    let provider = try XCTUnwrap(CGDataProvider(data: bytes as CFData))
    let image = try XCTUnwrap(CGImage(width: 2, height: 3, bitsPerComponent: 8, bitsPerPixel: 32,
                                      bytesPerRow: 8, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                                      provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
    let data = NSMutableData()
    let destination = try XCTUnwrap(CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil))
    CGImageDestinationAddImage(destination, image, [:] as CFDictionary)
    XCTAssertTrue(CGImageDestinationFinalize(destination))
    return data as Data
  }
}
