import CryptoKit
import Foundation
import RadrootsKit
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraLocalBackupTests: XCTestCase {
  func testForeignLeaseBindingRefusesBeforeCreatingBackupFiles() async throws {
    let fixture = try BackupFileFixture()
    defer { fixture.remove() }
    let host = try fixture.host()
    let foreign = FfiBackupMedia(sha256: String(repeating: "a", count: 64), byteLength: 20,
                                 leaseIdentifier: "another_backup")
    do {
      _ = try await host.retainMedia(request: fixture.request, media: [foreign])
      XCTFail("Canonical binding must be checked before native file effects")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "backup_media_unavailable") }
    XCTAssertFalse(FileManager.default.fileExists(atPath: fixture.roots.dataRoot.appendingPathComponent("backups").path))
  }

  func testActualOwnerCaptureAndNativePublicationAreStableAcrossRetry() async throws {
    let fixture = try BackupFileFixture()
    defer { fixture.remove() }
    let host = try fixture.host()
    try await host.prepare(request: fixture.request)
    _ = try RadrootsAppleMobileStore.prepare(roots: fixture.roots, publicKeyHex: fixture.request.publicKey,
                                             protectedDataAvailability: .available)
    let runtime = try await TeraRuntime.withHostSignerAndLocalBackups(
      applicationSupportDirectory: fixture.roots.dataRoot.path, publicKeyHex: fixture.request.publicKey,
      sourceGenerationHex: fixture.request.sourceGeneration, sourceGenerationCreatedAtUnixMs: 100,
      protectedData: .available, hostSigner: TeraGeneratedHostSigner(signer: TestRuntimeSigner())
    )
    let result = try await host.capture(runtime: runtime, request: fixture.request)
    XCTAssertEqual(result.request, fixture.request)
    XCTAssertFalse(result.manifest.isEmpty)
    let replay = try await host.capture(runtime: runtime, request: fixture.request)
    XCTAssertEqual(result, replay)
    XCTAssertEqual(try fixture.files.read(id: fixture.request.backupId, completed: true), result.manifest)
    _ = try await runtime.shutdown()
  }

  func testImmutableMediaLeaseSurvivesOriginalRemovalAndRejectsCorruption() async throws {
    let fixture = try BackupFileFixture()
    defer { fixture.remove() }
    let host = try fixture.host()
    try await host.prepare(request: fixture.request)
    let bytes = Data("backup media fixture".utf8)
    let hash = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    let blob = try RadrootsStagedBlobReference(blobID: hash, sizeBytes: bytes.count)
    let access = try RadrootsAppleFileAccess(roots: TeraDurableMediaRoots.selectingStaging(in: fixture.roots))
    try access.installStagedBlob(bytes, reference: blob)
    let media = FfiBackupMedia(sha256: hash, byteLength: UInt64(bytes.count),
                               leaseIdentifier: "tera_backup_\(fixture.request.backupId)_\(hash)")
    let retained = try await host.retainMedia(request: fixture.request, media: [media])
    XCTAssertEqual(retained, [media])
    try access.releaseStagedBlob(blob)
    let replay = try await host.retainMedia(request: fixture.request, media: [media])
    XCTAssertEqual(replay, retained)
    let directory = fixture.roots.dataRoot.appendingPathComponent("backups/\(fixture.request.publicKey)/staged_blob_leases")
    let lease = directory.appendingPathComponent(media.leaseIdentifier)
    XCTAssertEqual(try directory.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup, true)
    let attributes = try FileManager.default.attributesOfItem(atPath: lease.path)
    XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o400)
    XCTAssertFalse(FileManager.default.fileExists(atPath: fixture.roots.dataRoot.appendingPathComponent("staged_blob_leases").path))
    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: lease.path)
    let corrupt = Data(repeating: 0, count: bytes.count)
    try corrupt.write(to: lease)
    do {
      _ = try await host.retainMedia(request: fixture.request, media: [media])
      XCTFail("A corrupt retained lease must not be replaced")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "backup_media_unavailable") }
    XCTAssertEqual(try Data(contentsOf: lease), corrupt)
  }

  func testMissingMediaAndConflictingManifestCannotPublishCompletion() async throws {
    let fixture = try BackupFileFixture()
    defer { fixture.remove() }
    let host = try fixture.host()
    try await host.prepare(request: fixture.request)
    let hash = String(repeating: "a", count: 64)
    let media = FfiBackupMedia(sha256: hash, byteLength: 42, leaseIdentifier: "tera_backup_\(fixture.request.backupId)_\(hash)")
    do {
      _ = try await host.retainMedia(request: fixture.request, media: [media])
      XCTFail("Missing media cannot become a receipt")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "backup_media_unavailable") }
    // This fixture exercises only the file callback; canonical manifest policy
    // is Rust-owned and the end-to-end test above uses its real encoded bytes.
    let first = FfiBackupManifest(request: fixture.request, media: [], manifest: Data("original".utf8))
    try await host.persistCandidate(manifest: first)
    let conflict = FfiBackupManifest(request: fixture.request, media: [], manifest: Data("conflict".utf8))
    do {
      try await host.persistCandidate(manifest: conflict)
      XCTFail("An opaque manifest ID is create-only")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "backup_publication_incomplete") }
    do {
      try await host.publishComplete(manifest: conflict)
      XCTFail("A different candidate cannot be published")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "backup_publication_incomplete") }
    XCTAssertEqual(try fixture.files.read(id: fixture.request.backupId), first.manifest)
    XCTAssertNil(try fixture.files.read(id: fixture.request.backupId, completed: true))
  }

  func testProtectedDataIdentityCancellationAndSymlinkRefuseWithoutCandidate() async throws {
    let fixture = try BackupFileFixture()
    defer { fixture.remove() }
    let locked = try fixture.host(protectedData: { false })
    do {
      try await locked.prepare(request: fixture.request)
      XCTFail("Locked storage cannot be prepared")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "backup_unavailable") }
    XCTAssertFalse(FileManager.default.fileExists(atPath: fixture.roots.dataRoot.appendingPathComponent("backups").path))
    let foreign = try TeraLocalBackupHost(roots: fixture.roots, publicKey: String(repeating: "b", count: 64),
                                          generation: fixture.request.sourceGeneration, protectedData: { true })
    do {
      try await foreign.prepare(request: fixture.request)
      XCTFail("A foreign binding cannot create files")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "backup_identity_mismatch") }
    let host = try fixture.host()
    let cancelled = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      try await host.prepare(request: fixture.request)
    }
    do {
      try await cancelled.value
      XCTFail("Cancelled preparation cannot succeed")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "backup_publication_incomplete") }
    let outside = fixture.root.appendingPathComponent("outside")
    try FileManager.default.createDirectory(at: outside, withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: fixture.roots.dataRoot, withIntermediateDirectories: true)
    try FileManager.default.createSymbolicLink(at: fixture.roots.dataRoot.appendingPathComponent("backups"), withDestinationURL: outside)
    do { try await host.prepare(request: fixture.request); XCTFail("Symlink traversal is forbidden") } catch {}
    XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: outside.path), [])
  }
}

private struct BackupFileFixture {
  let root: URL
  let roots: RadrootsAppleFileRoots
  let request = FfiBackupRequest(schemaVersion: 1, backupId: String(repeating: "01", count: 16),
                                 publicKey: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                                 sourceGeneration: String(repeating: "07", count: 32), requestedAtUnixMs: 200, maximumBytes: 128 * 1024 * 1024)

  init() throws {
    root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
      .appendingPathComponent("tera-backup-\(UUID().uuidString)", isDirectory: true)
    roots = try RadrootsAppleFileRoots(appIdentifier: "test.tera.backup", dataRoot: root.appendingPathComponent("data"),
                                       cacheRoot: root.appendingPathComponent("cache"), temporaryRoot: root.appendingPathComponent("temporary"))
  }

  var files: TeraBackupFiles {
    TeraBackupFiles(roots: roots, publicKey: request.publicKey)
  }

  func host(protectedData: @escaping @Sendable () async -> Bool = { true }) throws -> TeraLocalBackupHost {
    try TeraLocalBackupHost(roots: roots, publicKey: request.publicKey, generation: request.sourceGeneration, protectedData: protectedData)
  }

  func remove() {
    try? FileManager.default.removeItem(at: root)
  }
}
