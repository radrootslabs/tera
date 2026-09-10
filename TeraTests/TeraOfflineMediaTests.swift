import CryptoKit
import RadrootsKit
@testable import TeraApp
import UIKit
import XCTest

@MainActor
final class TeraOfflineMediaTests: XCTestCase {
  func testUnobservedServiceKeepsLocalImportAndIncompleteMetadataSaveAvailable() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let client = TeraRuntimeClient.production()
    let signer = ComposerForbiddenSigner()
    let configuration = fixture.configuration(signer)
    let snapshot = try await client.start(configuration: configuration)
    // A nil launch override selects the public default; it is not an uplink probe.
    XCTAssertNotNil(snapshot.blossomConfiguration)
    let store = TeraAddStore(runtimeClient: client, media: fixture.coordinator())
    store.configure(snapshot: snapshot)
    await store.start()
    store.selectType(.createPhotoUpdate)
    XCTAssertEqual(store.mediaSupport, .init(library: true, camera: true))
    await store.importPhotos()
    let photo = try XCTUnwrap(store.form.media.first)
    XCTAssertNil(photo.remoteURL)
    XCTAssertEqual(photo.alt, "")
    let bytes = try Data(contentsOf: fixture.roots.stagedBlobsRoot.appendingPathComponent(photo.sha256))
    XCTAssertEqual(SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined(), photo.sha256)
    XCTAssertEqual(UInt64(bytes.count), photo.byteSize)
    store.updateForm(\.content, "  unfinished photo update  ")
    await store.save()
    let saved = try XCTUnwrap(store.savedComposer)
    XCTAssertEqual(saved.form.media.first?.alt, "")
    XCTAssertEqual(saved.form.media.first?.opaqueReference, photo.opaqueReference)
    await store.submit()
    XCTAssertEqual(store.lastFailureCode, "invalid_media_reference")
    XCTAssertNil(store.activeDraft)
    let operations = try await client.legacyDraftPage()
    XCTAssertTrue(operations.entries.isEmpty)
    let afterSave = try await client.snapshot()
    XCTAssertEqual(afterSave.blossomEvidence, snapshot.blossomEvidence)
    store.stop()
    _ = try await client.stop()
    try await assertRelaunch(fixture, client: client, configuration: configuration, saved: saved)
    let signs = await signer.requests
    let transfers = await fixture.transfer.enqueueCount
    XCTAssertEqual(signs, 0)
    XCTAssertEqual(transfers, 0)
    let requests = await fixture.picker.importRequests
    XCTAssertEqual(requests.count, 1)
    XCTAssertEqual(requests.first?.destinationScope, .cache)
    XCTAssertEqual(requests.first?.selectionLimit, 20)
  }

  func testAbsentServiceSnapshotKeepsLocalIntakeAndSaveWithoutProbing() async throws {
    let backend = try TeraScopeBackend()
    let original = await backend.value
    let snapshot = TeraRuntimeSnapshot(identity: original.identity, relay: original.relay,
                                       blossomConfiguration: nil, blossomEvidence: nil,
                                       crateName: original.crateName, crateVersion: original.crateVersion, isClosed: false)
    await backend.configure(snapshot)
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client, media: AddMediaHarness())
    store.configure(snapshot: snapshot)
    await store.start()
    store.selectType(.createPhotoUpdate)
    await store.checkPhotoService()
    XCTAssertEqual(store.mediaSupport, .init(library: true, camera: true))
    await store.importPhotos()
    let photo = try XCTUnwrap(store.form.media.first)
    store.updateMediaAlt(id: photo.id, alt: "")
    await store.save()
    XCTAssertEqual(store.savedComposer?.form.media.first?.alt, "")
    XCTAssertTrue(store.form.media.allSatisfy { $0.remoteURL == nil })
    let counts = await backend.counts
    XCTAssertNil(counts[.probe])
    XCTAssertNil(counts[.save])
    store.stop()
    _ = try await client.stop()
  }

  func testFailedServiceProbeDoesNotDisablePickerOrPersistAnUploadURL() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client, media: AddMediaHarness())
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)
    let failure = await backend.pause(.probe, fails: true)
    let probe = Task { await store.checkPhotoService() }
    await failure.entered.wait()
    await failure.resume.open()
    await probe.value
    XCTAssertEqual(store.mediaSupport, .init(library: true, camera: true))
    XCTAssertNotNil(store.message)
    await store.importPhotos()
    let photo = try XCTUnwrap(store.form.media.first)
    store.updateMediaAlt(id: photo.id, alt: "  unfinished description\n")
    await store.save()
    XCTAssertEqual(store.savedComposer?.form.media.first?.alt, "  unfinished description\n")
    XCTAssertTrue(store.form.media.allSatisfy { $0.remoteURL == nil })
    let counts = await backend.counts
    XCTAssertEqual(counts[.probe], 1)
    XCTAssertNil(counts[.save])
    store.stop()
    _ = try await client.stop()
  }

  func testCameraIntakeRemainsLocalAndDeniedPermissionPreservesExistingEditing() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client, media: fixture.coordinator())
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)
    await store.capturePhoto()
    let photo = try XCTUnwrap(store.form.media.first)
    XCTAssertNil(photo.remoteURL)
    XCTAssertEqual(photo.alt, "")
    let editing = store.form
    await fixture.picker.denyPermission()
    await store.importPhotos()
    XCTAssertEqual(store.form, editing)
    XCTAssertNotNil(store.message)
    XCTAssertNil(store.activeDraft)
    let requests = await fixture.picker.captureRequests
    XCTAssertEqual(requests.count, 1)
    XCTAssertEqual(requests.first?.destinationScope, .cache)
    let transfers = await fixture.transfer.enqueueCount
    XCTAssertEqual(transfers, 0)
    store.stop()
    _ = try await client.stop()
  }

  func testUnavailablePickerIsNotEnabledByConfiguredOrReachableService() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    await fixture.picker.setUnavailable()
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client, media: fixture.coordinator())
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    XCTAssertEqual(store.mediaSupport, .unavailable)
    await store.checkPhotoService()
    XCTAssertEqual(store.mediaSupport, .unavailable)
    store.stop()
    _ = try await client.stop()
  }

  private func assertRelaunch(_ fixture: OfflineMediaFixture, client: TeraRuntimeClient,
                              configuration: TeraRuntimeLaunchConfiguration, saved: TeraComposerDraft) async throws
  {
    let snapshot = try await client.start(configuration: configuration)
    let coordinator = fixture.coordinator()
    let store = TeraAddStore(runtimeClient: client, media: coordinator)
    store.configure(snapshot: snapshot)
    await store.start()
    let reopened = await store.reopenSaved(.composer(saved.id))
    XCTAssertTrue(reopened)
    XCTAssertEqual(store.savedComposer, saved)
    XCTAssertEqual(TeraComposerForm(editing: store.form), saved.form)
    let opened = try await coordinator.open(store.form.media)
    opened.close()
    let photo = try XCTUnwrap(store.form.media.first)
    store.updateMediaAlt(id: photo.id, alt: "Carrots prepared for the market")
    await store.save()
    XCTAssertEqual(store.savedComposer?.id, saved.id)
    XCTAssertEqual(store.savedComposer?.revision, saved.revision + 1)
    XCTAssertEqual(store.savedComposer?.form.media.first?.alt, "Carrots prepared for the market")
    let loaded = try await client.loadComposer(scope: saved.scope, id: saved.id)
    XCTAssertEqual(loaded, store.savedComposer)
    store.stop()
    _ = try await client.stop()
  }
}

@MainActor
private struct OfflineMediaFixture {
  let runtime: MediaOwnershipFixture
  let roots: RadrootsAppleFileRoots
  let picker: OfflineMediaPicker
  let transfer = BackgroundTransferHarness()

  init() throws {
    runtime = try MediaOwnershipFixture()
    roots = try RadrootsAppleFileRoots(appIdentifier: "test.offline-media",
                                       dataRoot: runtime.root.appendingPathComponent("media/data"),
                                       cacheRoot: runtime.root.appendingPathComponent("media/cache"),
                                       temporaryRoot: runtime.root.appendingPathComponent("media/temporary"))
    try FileManager.default.createDirectory(at: roots.cacheRoot, withIntermediateDirectories: true)
    let bytes = UIGraphicsImageRenderer(size: CGSize(width: 2, height: 2)).pngData { context in
      UIColor.green.setFill()
      context.fill(CGRect(x: 0, y: 0, width: 2, height: 2))
    }
    try bytes.write(to: roots.cacheRoot.appendingPathComponent("input.png"))
    picker = OfflineMediaPicker(byteSize: UInt64(bytes.count))
  }

  func coordinator() -> TeraAddMediaCoordinator {
    TeraAddMediaCoordinator(roots: roots, picker: picker, preparer: RadrootsAppleMediaPreparer(roots: roots),
                            transfer: transfer, clock: .fixed(unixSeconds: 1_800_000_000))
  }

  func configuration(_ signer: ComposerForbiddenSigner) -> TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: runtime.root.path,
      publicKeyHex: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
      sourceGenerationHex: String(repeating: "04", count: 32), sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
      protectedData: .available, networkProfile: .publicNetwork, writableRelays: [], blossom: nil,
      app: TeraRuntimeAppMetadata(bundleIdentifier: "test.offline-media", version: "1", buildNumber: "1", buildSHA: nil),
      signerGeneration: "offline-media-test", signer: signer, adoptBootstrapSettings: false
    )
  }

  func remove() {
    runtime.remove()
  }
}

private actor OfflineMediaPicker: RadrootsMediaPicker {
  let byteSize: UInt64
  private var denied = false
  private var available = true
  private(set) var importRequests: [RadrootsMediaImportRequest] = []
  private(set) var captureRequests: [RadrootsMediaCaptureRequest] = []

  init(byteSize: UInt64) {
    self.byteSize = byteSize
  }

  func denyPermission() {
    denied = true
  }

  func setUnavailable() {
    available = false
  }

  func currentSupport() throws -> RadrootsMediaPickerSupport {
    try RadrootsMediaPickerSupport(importAvailable: available, cameraCaptureAvailable: available,
                                   supportedImportKinds: [.image], supportedCaptureKinds: [.image], multipleSelectionSupported: true)
  }

  func importMedia(_ request: RadrootsMediaImportRequest) throws -> RadrootsMediaImportResult {
    importRequests.append(request)
    return try RadrootsMediaImportResult(items: [asset(.libraryImport)])
  }

  func captureMedia(_ request: RadrootsMediaCaptureRequest) throws -> RadrootsMediaCaptureResult {
    captureRequests.append(request)
    return try RadrootsMediaCaptureResult(item: asset(.cameraCapture))
  }

  private func asset(_ source: RadrootsMediaSource) throws -> RadrootsMediaAsset {
    guard !denied else { throw RadrootsCaptureIntakeError.permissionDenied }
    guard available else { throw RadrootsCaptureIntakeError.unavailable }
    return try RadrootsMediaAsset(source: source, kind: .image, file: RadrootsFileReference(scope: .cache, relativePath: "input.png"),
                                  mediaType: "image/png", suggestedFilename: "input.png", sizeBytes: byteSize,
                                  pixelWidth: 2, pixelHeight: 2, capturedAt: Date(timeIntervalSince1970: 1_800_000_000))
  }
}
