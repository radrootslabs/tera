import CryptoKit
import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraDurableMediaRootsTests: XCTestCase {
  func testVerifiedLegacyCopySurvivesTemporaryAndCacheRemoval() throws {
    let fixture = try DurableMediaFixture()
    defer { fixture.remove() }
    let roots = try TeraDurableMediaRoots.selectingStaging(in: fixture.legacy)
    XCTAssertEqual(roots.stagedBlobsRoot, roots.dataRoot.appendingPathComponent("staged_blobs", isDirectory: true))
    try TeraDurableMediaRoots.restoreLegacyBlob(fixture.blob, roots: roots)
    let access = RadrootsAppleFileAccess(roots: roots)
    XCTAssertEqual(try access.readStagedBlob(fixture.blob), fixture.bytes)
    XCTAssertEqual(try Data(contentsOf: fixture.legacyURL), fixture.bytes)
    try FileManager.default.removeItem(at: roots.temporaryRoot)
    try FileManager.default.removeItem(at: roots.cacheRoot)
    try TeraDurableMediaRoots.restoreLegacyBlob(fixture.blob, roots: roots)
    XCTAssertEqual(try access.readStagedBlob(fixture.blob), fixture.bytes)
  }

  func testCorruptAndSymlinkLegacySourcesCannotBecomeDurableMedia() throws {
    for symlink in [false, true] {
      let fixture = try DurableMediaFixture()
      defer { fixture.remove() }
      let roots = try TeraDurableMediaRoots.selectingStaging(in: fixture.legacy)
      try FileManager.default.removeItem(at: fixture.legacyURL)
      if symlink {
        let outside = fixture.root.appendingPathComponent("outside")
        try fixture.bytes.write(to: outside)
        try FileManager.default.createSymbolicLink(at: fixture.legacyURL, withDestinationURL: outside)
      } else {
        try Data(repeating: 0, count: fixture.bytes.count).write(to: fixture.legacyURL)
      }
      XCTAssertThrowsError(try TeraDurableMediaRoots.restoreLegacyBlob(fixture.blob, roots: roots))
      XCTAssertFalse(try FileManager.default.fileExists(atPath: roots.stagedBlobURL(for: fixture.blob).path))
    }
  }

  func testNewWorkUsesForegroundWhileExistingNativeReceiptRemainsDiscoverable() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let fresh = try await coordinator.prefersSharedForegroundUpload(ownerID: fixture.draftID)
    XCTAssertTrue(fresh)
    let request = try fixture.request(job: fixture.job(revision: 1, operation: String(repeating: "2", count: 32)))
    try await transfer.seed(request: request, state: .awaitingVerification)
    let retained = try await coordinator.prefersSharedForegroundUpload(ownerID: fixture.draftID)
    XCTAssertFalse(retained)
    let unrelated = try await coordinator.prefersSharedForegroundUpload(ownerID: String(repeating: "3", count: 32))
    XCTAssertTrue(unrelated)
    try await transfer.setState(.completed)
    let settled = try await coordinator.prefersSharedForegroundUpload(ownerID: fixture.draftID)
    XCTAssertTrue(settled)
    let count = await transfer.enqueueCount
    XCTAssertEqual(count, 0)
  }
}

private struct DurableMediaFixture {
  let root: URL
  let legacy: RadrootsAppleFileRoots
  let blob: RadrootsStagedBlobReference
  let bytes = Data("acknowledged prepared bytes".utf8)
  var legacyURL: URL {
    legacy.stagedBlobsRoot.appendingPathComponent(blob.blobID)
  }

  init() throws {
    root = FileManager.default.temporaryDirectory.appendingPathComponent("tera-durable-media-\(UUID().uuidString)", isDirectory: true)
    legacy = try RadrootsAppleFileRoots(
      appIdentifier: "test.tera.media", dataRoot: root.appendingPathComponent("data", isDirectory: true),
      cacheRoot: root.appendingPathComponent("cache", isDirectory: true),
      temporaryRoot: root.appendingPathComponent("temporary", isDirectory: true)
    )
    let digest = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    blob = try RadrootsStagedBlobReference(blobID: digest, sizeBytes: bytes.count, mediaType: "image/png", filenameHint: "\(digest).png")
    try FileManager.default.createDirectory(at: legacy.stagedBlobsRoot, withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: legacy.cacheRoot, withIntermediateDirectories: true)
    try bytes.write(to: legacyURL)
  }

  func remove() {
    try? FileManager.default.removeItem(at: root)
  }
}
