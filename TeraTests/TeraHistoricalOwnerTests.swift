import Darwin
import Foundation
import RadrootsKit
@testable import TeraApp
import TeraKitBindings
import XCTest

@MainActor
final class TeraHistoricalOwnerTests: XCTestCase {
  func testHistoricalConfigurationMediaAndNativeReceiptKeepTheirOriginalIdentities() async throws {
    let fixture = try TeraHistoricalOwnerFixture()
    defer { fixture.remove() }
    let roots = try fixture.roots()
    let configuration = try TeraConfigurationStore(bootstrap: fixture.bootstrap(), roots: roots, clock: .fixed(unixSeconds: 1_700_000_000))
    let selected = try await configuration.load()
    XCTAssertEqual(selected.bundleIdentifier, try fixture.text(fixture.host, "bundle_identifier"))
    XCTAssertEqual(selected.keychainServicePrefix, try fixture.text(fixture.host, "keychain_service_prefix"))
    XCTAssertEqual(selected.generation, 1)
    let generation = try await configuration.sourceGeneration()
    XCTAssertEqual(generation.generationHex, try fixture.generation)
    XCTAssertEqual(generation.createdAtUnixMilliseconds, try fixture.number(fixture.host, "generation_created_at_unix_ms"))
    let blob = try fixture.blob()
    let legacy = try fixture.root.appendingPathComponent(fixture.text(fixture.host, "staged_relative_path"))
    let original = try Data(contentsOf: legacy)
    try TeraDurableMediaRoots.restoreLegacyBlob(blob, roots: roots)
    XCTAssertEqual(try RadrootsAppleFileAccess(roots: roots).readStagedBlob(blob), original)
    XCTAssertEqual(try Data(contentsOf: legacy), original)
    let store = RadrootsAppleBackgroundTransferStore(roots: roots)
    let snapshots = try await store.loadSnapshots()
    XCTAssertEqual(snapshots.count, 1)
    let snapshot = try XCTUnwrap(snapshots.first)
    XCTAssertEqual(snapshot.identifier.rawValue, try fixture.text(fixture.native, "native_identifier"))
    XCTAssertEqual(snapshot.state, .awaitingVerification)
    XCTAssertEqual(snapshot.request.headers, [:])
    XCTAssertEqual(snapshot.request.expectedSourceSHA256, blob.blobID)
    try FileManager.default.removeItem(at: roots.temporaryRoot)
    try FileManager.default.removeItem(at: roots.cacheRoot)
    try TeraDurableMediaRoots.restoreLegacyBlob(blob, roots: roots)
    XCTAssertEqual(try RadrootsAppleFileAccess(roots: roots).readStagedBlob(blob), original)
    let reopened = try await RadrootsAppleBackgroundTransferStore(roots: roots).loadSnapshots()
    XCTAssertEqual(reopened, snapshots)
    // Typed historical persistence is read without instantiating an OS driver.
  }

  func testInstalledCurrentBinaryReadsAndReopensAllHistoricalAcknowledgedDraftsWithoutSigner() async throws {
    let fixture = try TeraHistoricalOwnerFixture()
    defer { fixture.remove() }
    for _ in 0 ..< 2 {
      let runtime = try await fixture.runtime()
      try await fixture.assertStatuses(runtime)
      let signed = try await runtime.phase1DraftStatus(draftId: String(repeating: "0", count: 31) + "3")
      XCTAssertEqual(signed.settlement?.signed, 1)
      // The historical owner signed this work and retained pending delivery;
      // migration must preserve its original zero local-admission count.
      XCTAssertEqual(signed.settlement?.admitted, 0)
      XCTAssertEqual(signed.settlement?.pending, 1)
      XCTAssertEqual(signed.settlement?.deliveryPlans, 1)
      _ = try await runtime.shutdown()
    }
  }

  func testNativeColdRestoreOfHistoricalOutboxRetainsMediaAndCannotGrantResumeWithoutReview() async throws {
    let fixture = try TeraHistoricalOwnerFixture()
    defer { fixture.remove() }
    let roots = try fixture.roots()
    let blob = try fixture.blob()
    try TeraDurableMediaRoots.restoreLegacyBlob(blob, roots: roots)
    let signer = ComposerForbiddenSigner()
    let backup = try await captureHistoricalBackup(fixture, roots: roots, blob: blob, signer: signer)
    // This isolated fixture now simulates a fresh process after actual close.
    // Production never removes its live PID marker to grant cold admission.
    let access = RadrootsAppleFileAccess(roots: roots)
    try access.delete(RadrootsFileReference(scope: .data, relativePath: "\(TeraMediaProcessUse.directory)/\(getpid())"))
    try FileManager.default.removeItem(at: roots.stagedBlobURL(for: blob))
    let store = try FfiRestoreStore(applicationSupportDirectory: roots.dataRoot.path, publicKey: backup.publicKey,
                                    sourceGeneration: backup.sourceGeneration, sourceGenerationCreatedAtMs: fixture.number(fixture.host, "generation_created_at_unix_ms"), protectedData: .available)
    let request = try FfiRestoreRequest(attemptId: String(repeating: "18", count: 16), backup: backup,
                                        requestedAtMs: TeraClock.system.unixMilliseconds(requirePositive: true))
    let restorer = try TeraLocalRestoreHost(roots: roots, activeTransfers: { [] }, protectedData: { true })
    let guardRecord = try await restorer.restore(store: store, request: request)
    XCTAssertEqual(try TeraHistoricalOwnerFixture.hash(access.readStagedBlob(blob)), blob.blobID)
    do { _ = try await fixture.runtime(); XCTFail("Ordinary startup must refuse a retained restore guard") } catch {}
    let restored = try await TeraRuntime.withHostSignerAndRestoreGuard(store: store,
                                                                       hostSigner: TeraGeneratedHostSigner(signer: signer), guard: guardRecord.bytes, localBackups: true)
    try await fixture.assertStatuses(restored)
    let status = try await restored.applicationRestoreStatus()
    XCTAssertEqual(status?.phase, .held)
    XCTAssertEqual(status?.targets.count, 4)
    do {
      _ = try await restored.reviewRestoredWork()
      XCTFail("Historical local provenance cannot grant a fresh target review")
    } catch {
      XCTAssertEqual(TeraGeneratedRuntimeFailure.from(error).code, "restore_reconciliation_required")
    }
    try await fixture.assertStatuses(restored)
    _ = try await restored.shutdown()
    let requests = await signer.requests
    XCTAssertEqual(requests, 0)
  }

  private func captureHistoricalBackup(_ fixture: TeraHistoricalOwnerFixture, roots: RadrootsAppleFileRoots,
                                       blob: RadrootsStagedBlobReference, signer: ComposerForbiddenSigner) async throws -> FfiBackupRequest
  {
    let now = try TeraClock.system.unixMilliseconds(requirePositive: true)
    let backup = try FfiBackupRequest(schemaVersion: 1, backupId: String(repeating: "17", count: 16), publicKey: fixture.publicKey,
                                      sourceGeneration: fixture.generation, requestedAtUnixMs: now, maximumBytes: 128 * 1024 * 1024)
    let host = try TeraLocalBackupHost(roots: roots, publicKey: backup.publicKey, generation: backup.sourceGeneration, protectedData: { true })
    try await host.prepare(request: backup)
    let runtime = try await TeraRuntime.withHostSignerAndLocalBackups(applicationSupportDirectory: roots.dataRoot.path,
                                                                      publicKeyHex: backup.publicKey, sourceGenerationHex: backup.sourceGeneration,
                                                                      sourceGenerationCreatedAtUnixMs: fixture.number(fixture.host, "generation_created_at_unix_ms"),
                                                                      protectedData: .available, hostSigner: TeraGeneratedHostSigner(signer: signer))
    try await fixture.assertStatuses(runtime)
    let captured = try await host.capture(runtime: runtime, request: backup)
    XCTAssertEqual(captured.media.count, 1)
    XCTAssertEqual(captured.media.first?.sha256, blob.blobID)
    _ = try await runtime.shutdown()
    return backup
  }
}
