import Darwin
import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraMediaCleanupTests: XCTestCase {
  func testInvalidRootCannotCreateAnAdmissionMarker() {
    for path in ["", "relative", "/tmp/../unknown", "/tmp/./unknown", "/"] {
      XCTAssertThrowsError(try TeraMediaProcessUse.admit(applicationSupportDirectory: path))
    }
  }

  func testProtectedDataLossDuringInventoryRetainsEveryFile() async throws {
    let fixture = try MediaCleanupFixture()
    defer { fixture.remove() }
    let old = try fixture.stage(String(repeating: "a", count: 64))
    let availability = MediaCleanupAvailability()
    let result = await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now, protectedData: { await availability.firstOnly() })
    XCTAssertEqual(result, .retained)
    XCTAssertTrue(fixture.exists(old))
  }

  func testFailedRuntimeCreationLeavesItsProcessProtectionInPlace() async throws {
    let fixture = try MediaCleanupFixture()
    defer { fixture.remove() }
    let old = try fixture.stage(String(repeating: "a", count: 64))
    let configuration = TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: fixture.roots.dataRoot.path, publicKeyHex: "invalid",
      sourceGenerationHex: String(repeating: "04", count: 32), sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
      protectedData: .available, networkProfile: .publicNetwork, writableRelays: [], blossom: nil,
      app: TeraRuntimeAppMetadata(bundleIdentifier: "test.cleanup", version: "1", buildNumber: "1", buildSHA: nil),
      signerGeneration: "cleanup", signer: ComposerForbiddenSigner(), adoptBootstrapSettings: false
    )
    do {
      _ = try await TeraGeneratedRuntimeBackend.start(configuration: configuration)
      XCTFail("Invalid identity must fail runtime creation")
    } catch {}
    let result = await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now)
    XCTAssertEqual(result, .retained)
    XCTAssertTrue(fixture.exists(old))
  }

  func testLifecycleStartupFinishesCleanupBeforeRecordingNativeSessionUse() async throws {
    let fixture = try MediaCleanupFixture()
    defer { fixture.remove() }
    let old = try fixture.stage(String(repeating: "a", count: 64))
    try FileManager.default.setAttributes([.modificationDate: Date(timeIntervalSince1970: 1)], ofItemAtPath: old.path)
    let buffer = TeraDiagnosticsBuffer()
    let coordinator = TeraLifecycleCoordinator(
      telemetry: buffer, buffer: buffer, fileAccess: RadrootsAppleFileAccess(roots: fixture.roots),
      transfer: BackgroundTransferHarness(), transferIdentifier: "test.tera.cleanup.transfer"
    )
    async let first = coordinator.attachBackgroundEvents()
    async let second = coordinator.attachBackgroundEvents()
    let results = await (first, second)
    XCTAssertTrue(results.0)
    XCTAssertTrue(results.1)
    XCTAssertFalse(fixture.exists(old))
    XCTAssertTrue(fixture.exists(fixture.roots.dataRoot.appendingPathComponent("\(TeraMediaProcessUse.directory)/\(getpid())")))
    await TeraBackgroundEventRouter.shared.detachAndCompletePending()
  }

  func testColdPassCollectsOldOrphansAndScratchButPreservesFreshOpaqueAndLeaseFiles() async throws {
    let fixture = try MediaCleanupFixture()
    defer { fixture.remove() }
    let old = try fixture.stage(String(repeating: "a", count: 64))
    let fresh = try fixture.stage(String(repeating: "b", count: 64), old: false)
    let opaque = try fixture.stage("unknown")
    let scratch = try fixture.stage(".radroots_pending_12345678-1234-1234-1234-123456789abc")
    let lease = fixture.roots.dataRoot.appendingPathComponent("staged_blob_leases/lease")
    try FileManager.default.createDirectory(at: lease.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data([1]).write(to: lease)
    let result = await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now)
    XCTAssertEqual(result, .completed(removed: 2, exhaustedBudget: false))
    XCTAssertFalse(fixture.exists(old))
    XCTAssertFalse(fixture.exists(scratch))
    XCTAssertTrue(fixture.exists(fresh))
    XCTAssertTrue(fixture.exists(opaque))
    XCTAssertTrue(fixture.exists(lease))
  }

  func testProcessMarkerOutlivesDroppedAdmissionAndRetainsDetachedWork() async throws {
    let fixture = try MediaCleanupFixture()
    defer { fixture.remove() }
    let old = try fixture.stage(String(repeating: "a", count: 64))
    do {
      let use = try TeraMediaProcessUse.admit(root: fixture.roots.dataRoot)
      XCTAssertNil(try RadrootsAppleFileMaintenance(root: fixture.roots.dataRoot).reserveMaintenance())
      withExtendedLifetime(use) {}
    }
    // The descriptor has gone, as in a failed factory or cancelled caller;
    // the live process marker still prevents destructive absence inference.
    let result = await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now)
    XCTAssertEqual(result, .retained)
    XCTAssertTrue(fixture.exists(old))
  }

  func testExclusivePassRejectsNewAdmissionBeforeAnyProcessMarkerIsWritten() throws {
    let fixture = try MediaCleanupFixture()
    defer { fixture.remove() }
    let reservation = try XCTUnwrap(RadrootsAppleFileMaintenance(root: fixture.roots.dataRoot).reserveMaintenance())
    defer { withExtendedLifetime(reservation) {} }
    XCTAssertThrowsError(try TeraMediaProcessUse.admit(root: fixture.roots.dataRoot))
    XCTAssertFalse(fixture.exists(fixture.roots.dataRoot.appendingPathComponent(TeraMediaProcessUse.directory)))
  }

  func testOnlyDefinitelyAbsentWellFormedProcessMarkersCanBeRetired() throws {
    let fixture = try MediaCleanupFixture()
    defer { fixture.remove() }
    let access = RadrootsAppleFileAccess(roots: fixture.roots)
    let file = RadrootsFileReference(scope: .data, relativePath: "\(TeraMediaProcessUse.directory)/123")
    try access.write(.inline(Data("tera.media.use.v1 123\n".utf8)), to: file)
    let reservation = try XCTUnwrap(RadrootsAppleFileMaintenance(root: fixture.roots.dataRoot).reserveMaintenance())
    defer { withExtendedLifetime(reservation) {} }
    var remaining = 64
    XCTAssertFalse(try TeraMediaProcessUse.hasNoLiveUsers(roots: fixture.roots, scan: reservation.openDirectory(relativePath: TeraMediaProcessUse.directory), remaining: &remaining, isAbsent: { _ in false }))
    remaining = 64
    XCTAssertTrue(try TeraMediaProcessUse.hasNoLiveUsers(roots: fixture.roots, scan: reservation.openDirectory(relativePath: TeraMediaProcessUse.directory), remaining: &remaining, isAbsent: { $0 == 123 }))
    try access.write(.inline(Data("unknown version".utf8)), to: file)
    remaining = 64
    XCTAssertFalse(try TeraMediaProcessUse.hasNoLiveUsers(roots: fixture.roots, scan: reservation.openDirectory(relativePath: TeraMediaProcessUse.directory), remaining: &remaining, isAbsent: { _ in true }))
  }

  func testActiveUploadAndUnverifiedOrCancelledNativeReceiptKeepTheirSharedSource() async throws {
    for state in [RadrootsBackgroundTransferState.running, .awaitingVerification, .cancelled, .completed] {
      let fixture = try MediaCleanupFixture()
      let upload = try BackgroundUploadFixture()
      defer { fixture.remove(); upload.remove() }
      let source = try fixture.stage(upload.media.sha256)
      let orphan = try fixture.stage(String(repeating: "e", count: 64))
      let request = try upload.request(job: upload.job(revision: 1, operation: String(repeating: "2", count: 32)))
      let snapshot = try RadrootsBackgroundTransferSnapshot(request: request, state: state)
      try await RadrootsAppleBackgroundTransferStore(roots: fixture.roots).saveSnapshot(snapshot)
      let result = await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now)
      XCTAssertEqual(result, .completed(removed: 1, exhaustedBudget: false))
      XCTAssertTrue(fixture.exists(source))
      XCTAssertFalse(fixture.exists(orphan))
    }
  }

  func testUnknownTransferEvidenceAndBackupRootPreventCollection() async throws {
    for transfer in [false, true] {
      let fixture = try MediaCleanupFixture()
      defer { fixture.remove() }
      let old = try fixture.stage(String(repeating: "a", count: 64))
      if transfer {
        try RadrootsAppleFileAccess(roots: fixture.roots).write(.inline(Data("unknown".utf8)), to: RadrootsFileReference(scope: .data, relativePath: "background_transfers/transfers.json"))
      } else {
        try FileManager.default.createDirectory(at: fixture.roots.dataRoot.appendingPathComponent("backups"), withIntermediateDirectories: true)
      }
      let result = await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now)
      XCTAssertEqual(result, .retained)
      XCTAssertTrue(fixture.exists(old))
    }
  }

  func testCancelledPassRetainsFilesAndFreshPassRebuildsProof() async throws {
    let fixture = try MediaCleanupFixture()
    defer { fixture.remove() }
    let old = try fixture.stage(String(repeating: "a", count: 64))
    let task = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      return await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now)
    }
    let cancelled = await task.value
    XCTAssertEqual(cancelled, .retained)
    XCTAssertTrue(fixture.exists(old))
    let fresh = await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now)
    XCTAssertEqual(fresh, .completed(removed: 1, exhaustedBudget: false))
  }

  func testRemovalQuotaStopsBoundedlyAndNextPassReconcilesTheRemainder() async throws {
    let fixture = try MediaCleanupFixture()
    defer { fixture.remove() }
    for value in 1 ... 70 {
      _ = try fixture.stage(String(format: "%064x", value))
    }
    let first = await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now)
    XCTAssertEqual(first, .completed(removed: 64, exhaustedBudget: true))
    let second = await TeraMediaCleanup.run(roots: fixture.roots, now: fixture.now)
    XCTAssertEqual(second, .completed(removed: 6, exhaustedBudget: false))
  }
}

struct MediaCleanupFixture: Sendable {
  let root: URL
  let roots: RadrootsAppleFileRoots
  let now = Date(timeIntervalSince1970: 1_800_000_000)

  init() throws {
    root = FileManager.default.temporaryDirectory.appendingPathComponent("tera-media-cleanup-\(UUID().uuidString)", isDirectory: true)
    roots = try TeraDurableMediaRoots.selectingStaging(in: RadrootsAppleFileRoots(
      appIdentifier: "test.tera.cleanup", dataRoot: root.appendingPathComponent("data", isDirectory: true),
      cacheRoot: root.appendingPathComponent("cache", isDirectory: true), temporaryRoot: root.appendingPathComponent("temporary", isDirectory: true)
    ))
    try FileManager.default.createDirectory(at: roots.stagedBlobsRoot, withIntermediateDirectories: true)
  }

  func stage(_ name: String, old: Bool = true) throws -> URL {
    let url = roots.stagedBlobsRoot.appendingPathComponent(name)
    try Data([1, 2, 3]).write(to: url)
    try FileManager.default.setAttributes([.modificationDate: old ? now.addingTimeInterval(-8 * 86400) : now], ofItemAtPath: url.path)
    return url
  }

  func exists(_ url: URL) -> Bool {
    FileManager.default.fileExists(atPath: url.path)
  }

  func remove() {
    try? FileManager.default.removeItem(at: root)
  }
}

private actor MediaCleanupAvailability {
  private var calls = 0
  func firstOnly() -> Bool {
    calls += 1
    return calls == 1
  }
}
