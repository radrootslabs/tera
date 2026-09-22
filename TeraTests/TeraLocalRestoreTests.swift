import CryptoKit
import Darwin
import Foundation
import RadrootsKit
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraLocalRestoreTests: XCTestCase {
  func testActualColdRestoreAndGuardedReopenRequireExplicitResume() async throws {
    let fixture = try RestoreFileFixture()
    defer { fixture.remove() }
    let backup = try TeraLocalBackupHost(roots: fixture.roots, publicKey: fixture.backup.publicKey,
                                         generation: fixture.backup.sourceGeneration, protectedData: { true })
    try await backup.prepare(request: fixture.backup)
    let initial = try await fixture.runtime()
    _ = try await backup.capture(runtime: initial, request: fixture.backup)
    _ = try await initial.shutdown()
    let host = try fixture.host()
    do {
      _ = try await host.restore(store: fixture.store, request: fixture.request)
      XCTFail("A live process marker must block even after runtime shutdown")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "restore_busy") }
    // Simulate a fresh process in this test fixture only, after explicit close.
    // Production never removes a live PID marker to force recovery admission.
    try RadrootsAppleFileAccess(roots: fixture.roots).delete(RadrootsFileReference(
      scope: .data, relativePath: "\(TeraMediaProcessUse.directory)/\(getpid())"
    ))
    let guardRecord = try await host.restore(store: fixture.store, request: fixture.request)
    XCTAssertEqual(try TeraRestoreFiles(roots: fixture.roots, publicKey: fixture.backup.publicKey).readGuard(), guardRecord.bytes)
    do { _ = try await fixture.runtime(); XCTFail("Ordinary startup must refuse the guard") } catch {}
    let startup = try await TeraGeneratedRuntimeBackend.start(configuration: fixture.configuration, localBackups: true)
    let settings = try await startup.backend.mobileSettings()
    XCTAssertEqual(settings.revision, 1, "Guarded startup must not adopt conflicting launch settings")
    XCTAssertTrue(settings.identity.identities.isEmpty)
    let held = try await startup.backend.restoreStatus()
    XCTAssertEqual(held?.phase, .held)
    _ = try await startup.backend.shutdown()
    let restored = try await TeraRuntime.withHostSignerAndRestoreGuard(
      store: fixture.store, hostSigner: TeraGeneratedHostSigner(signer: TestRuntimeSigner()), guard: guardRecord.bytes, localBackups: true
    )
    let status = try await restored.applicationRestoreStatus()
    XCTAssertEqual(status?.phase, .held)
    XCTAssertEqual(status?.attemptId, fixture.request.attemptId)
    do {
      try await restored.resumeRestoredWork(reviewedInventory: String(repeating: "00", count: 32))
      XCTFail("Restore alone cannot authorize work")
    } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "restore_reconciliation_required") }
    let digest = try await restored.reviewRestoredWork()
    try await restored.resumeRestoredWork(reviewedInventory: digest)
    let resumed = try await restored.applicationRestoreStatus()
    XCTAssertEqual(resumed?.phase, .resumed)
    _ = try await restored.shutdown()
  }

  func testUnknownOrActiveNativeTasksAndProtectedDataRefuseBeforeGuard() async throws {
    let fixture = try RestoreFileFixture()
    defer { fixture.remove() }
    for active in [false, true] {
      let host = try TeraLocalRestoreHost(roots: fixture.roots, activeTransfers: {
        if active {
          return try [RadrootsBackgroundTransferIdentifier("test.active")]
        }
        throw RadrootsBackgroundTransferError.unavailable
      }, protectedData: { true })
      do {
        _ = try await host.restore(store: fixture.store, request: fixture.request)
        XCTFail("Unproven native inactivity cannot restore")
      } catch { XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, active ? "restore_busy" : "restore_recovery_required") }
      XCTAssertNil(try TeraRestoreFiles(roots: fixture.roots, publicKey: fixture.backup.publicKey).readGuard())
    }
    let locked = try TeraLocalRestoreHost(roots: fixture.roots, activeTransfers: { [] }, protectedData: { false })
    do { _ = try await locked.restore(store: fixture.store, request: fixture.request); XCTFail("Protected data must be available") } catch {}
    XCTAssertNil(try TeraRestoreFiles(roots: fixture.roots, publicKey: fixture.backup.publicKey).readGuard())
  }

  func testImmutableLeaseRestoresMissingMediaAndRefusesTamperedBytes() throws {
    let fixture = try RestoreFileFixture()
    defer { fixture.remove() }
    let roots = try TeraDurableMediaRoots.selectingStaging(in: fixture.roots)
    let bytes = Data("restore exact media".utf8)
    let hash = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    let item = FfiBackupMedia(sha256: hash, byteLength: UInt64(bytes.count),
                              leaseIdentifier: "tera_backup_\(fixture.backup.backupId)_\(hash)")
    let blob = try RadrootsStagedBlobReference(blobID: hash, sizeBytes: bytes.count)
    let access = RadrootsAppleFileAccess(roots: roots)
    try access.installStagedBlob(bytes, reference: blob)
    let backup = TeraBackupFiles(roots: roots, publicKey: fixture.backup.publicKey)
    let lease = try backup.mediaAccess().leaseStagedBlob(blob, expectedSHA256: hash, identifier: item.leaseIdentifier)
    try access.releaseStagedBlob(blob)
    let files = TeraRestoreFiles(roots: roots, publicKey: fixture.backup.publicKey)
    let manifest = FfiBackupManifest(request: fixture.backup, media: [item], manifest: Data())
    try files.restoreMedia(manifest)
    XCTAssertEqual(try access.readStagedBlob(blob), bytes)
    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: lease.fileURL.path)
    try Data(repeating: 0, count: bytes.count).write(to: lease.fileURL)
    XCTAssertThrowsError(try files.restoreMedia(manifest))
    XCTAssertEqual(try access.readStagedBlob(blob), bytes)
  }
}

private struct RestoreFileFixture {
  let root: URL
  let roots: RadrootsAppleFileRoots
  let backup = FfiBackupRequest(schemaVersion: 1, backupId: String(repeating: "01", count: 16),
                                publicKey: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                                sourceGeneration: String(repeating: "07", count: 32), requestedAtUnixMs: 200, maximumBytes: 128 * 1024 * 1024)
  init() throws {
    root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("tera-restore-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    roots = try RadrootsAppleFileRoots(appIdentifier: "test.tera.restore", dataRoot: root.appendingPathComponent("data"),
                                       cacheRoot: root.appendingPathComponent("cache"), temporaryRoot: root.appendingPathComponent("temporary"))
    _ = try RadrootsAppleMobileStore.prepare(roots: roots, publicKeyHex: backup.publicKey, protectedDataAvailability: .available)
    try TeraBackupFiles(roots: roots, publicKey: backup.publicKey).prepare()
  }

  var store: FfiRestoreStore {
    FfiRestoreStore(applicationSupportDirectory: roots.dataRoot.path, publicKey: backup.publicKey,
                    sourceGeneration: backup.sourceGeneration, sourceGenerationCreatedAtMs: 100, protectedData: .available)
  }

  var request: FfiRestoreRequest {
    FfiRestoreRequest(attemptId: String(repeating: "02", count: 16), backup: backup, requestedAtMs: 300)
  }

  var configuration: TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: roots.dataRoot.path, publicKeyHex: backup.publicKey,
      sourceGenerationHex: backup.sourceGeneration, sourceGenerationCreatedAtUnixMilliseconds: 100,
      protectedData: .available, networkProfile: .simulator, writableRelays: ["ws://127.0.0.1:19999"], blossom: nil,
      app: TeraRuntimeAppMetadata(bundleIdentifier: "test.restore", version: "1", buildNumber: "1", buildSHA: nil),
      signerGeneration: "restore", signer: TestRuntimeSigner(), adoptBootstrapSettings: true
    )
  }

  func host() throws -> TeraLocalRestoreHost {
    try TeraLocalRestoreHost(roots: roots, activeTransfers: { [] }, protectedData: { true })
  }

  func runtime() async throws -> TeraRuntime {
    try await TeraRuntime.withHostSignerAndLocalBackups(applicationSupportDirectory: roots.dataRoot.path,
                                                        publicKeyHex: backup.publicKey, sourceGenerationHex: backup.sourceGeneration,
                                                        sourceGenerationCreatedAtUnixMs: 100, protectedData: .available,
                                                        hostSigner: TeraGeneratedHostSigner(signer: TestRuntimeSigner()))
  }

  func remove() {
    try? FileManager.default.removeItem(at: root)
  }
}
