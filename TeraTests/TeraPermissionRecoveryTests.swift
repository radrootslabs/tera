import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraPermissionRecoveryTests: XCTestCase {
  func testSharedCoordinatorKeepsLibraryIndependentAndNeverCapturesDeniedRestrictedOrUnavailable() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    for access in TeraCameraAccess.allCases {
      let coordinator = fixture.coordinator(cameraAccess: { access })
      let support = try await coordinator.support()
      XCTAssertTrue(support.library)
      XCTAssertEqual(support.cameraAccess, access)
      XCTAssertEqual(support.camera, access == .authorized || access == .notDetermined)
      let imported = try await coordinator.importImages(limit: 1)
      XCTAssertEqual(imported.count, 1)
      if support.camera {
        let photo = try await coordinator.captureImage()
        XCTAssertNil(photo.remoteURL)
      } else {
        do {
          _ = try await coordinator.captureImage()
          XCTFail("Denied capability cannot call the picker")
        } catch { XCTAssertTrue(error is TeraRuntimeFailure) }
      }
    }
    let captures = await fixture.picker.captureRequests
    let imports = await fixture.picker.importRequests
    let uploads = await fixture.transfer.enqueueCount
    XCTAssertEqual(captures.count, 2)
    XCTAssertEqual(imports.count, 5)
    XCTAssertEqual(uploads, 0)
  }

  func testDeniedCameraPreservesEditingAndSaveUntilExplicitSettingsRecheck() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let permission = CameraPermissionProbe(.denied)
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client, media: fixture.coordinator(cameraAccess: permission.read))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)
    store.updateForm(\.content, "Keep this unfinished description")
    let original = store.form
    await store.capturePhoto()
    await store.capturePhoto()
    XCTAssertEqual(store.form, original)
    XCTAssertEqual(store.mediaSupport.cameraAccess, .denied)
    XCTAssertFalse(store.mediaSupport.camera)
    XCTAssertTrue(store.mediaSupport.library)
    await store.save()
    XCTAssertEqual(store.savedComposer?.form.content, original.content)
    let deniedCaptures = await fixture.picker.captureRequests
    XCTAssertTrue(deniedCaptures.isEmpty)
    permission.set(.authorized)
    await store.recheckMediaAccess()
    XCTAssertTrue(store.mediaSupport.camera)
    XCTAssertEqual(store.form, original)
    await store.capturePhoto()
    XCTAssertEqual(store.form.media.count, 1)
    XCTAssertEqual(store.form.content, original.content)
    XCTAssertNil(store.form.media.first?.remoteURL)
    let captures = await fixture.picker.captureRequests
    XCTAssertEqual(captures.count, 1)
    store.stop()
    _ = try await client.stop()
  }

  func testLatePermissionRecheckCannotOverwriteNewerReadOrChangedAccount() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let media = PausedPermissionMedia()
    let store = TeraAddStore(runtimeClient: client, media: media)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    let first = ResourceTestPause()
    await media.hold(first, access: .denied)
    let old = Task { await store.recheckMediaAccess() }
    await first.entered.wait()
    await store.recheckMediaAccess()
    XCTAssertTrue(store.mediaSupport.camera)
    await first.resume.open()
    await old.value
    XCTAssertTrue(store.mediaSupport.camera)
    let second = ResourceTestPause()
    await media.hold(second, access: .authorized)
    let stale = Task { await store.recheckMediaAccess() }
    await second.entered.wait()
    store.configure(snapshot: TeraScopeFixtures.snapshot(account: "b"))
    await second.resume.open()
    await stale.value
    XCTAssertEqual(store.mediaSupport, .unavailable)
    XCTAssertNil(store.message)
    store.stop()
    _ = try await client.stop()
  }

  func testLocalStateRecheckUsesRetryWithoutSubstitutingCustodyActions() {
    let identity = TeraAppIdentity(state: .locked, identityHandle: nil, publicKeyHex: nil,
                                   label: nil, signerGeneration: nil, recoveryCode: nil)
    for phase: TeraSessionPhase in [.protectedDataUnavailable(identity), .corruptIdentity(identity), .stopped] {
      XCTAssertTrue(status(phase).canRecheckLocalState)
    }
    for phase: TeraSessionPhase in [.starting, .identityRequired, .identityLocked(identity), .recoveryRequired(identity)] {
      XCTAssertFalse(status(phase).canRecheckLocalState)
    }
  }

  private func status(_ phase: TeraSessionPhase) -> RuntimeStatusView {
    RuntimeStatusView(phase: phase, retry: {}, createIdentity: {}, importIdentity: { _ in },
                      unlockIdentity: {}, recoverIdentity: {}, applyConfigurationReconfiguration: {})
  }
}

private final class CameraPermissionProbe: @unchecked Sendable {
  private let lock = NSLock()
  private var value: TeraCameraAccess
  init(_ value: TeraCameraAccess) {
    self.value = value
  }

  func read() -> TeraCameraAccess {
    lock.withLock { value }
  }

  func set(_ value: TeraCameraAccess) {
    lock.withLock { self.value = value }
  }
}

private actor PausedPermissionMedia: TeraAddMediaHandling {
  private var pending: (ResourceTestPause, TeraCameraAccess)?
  func hold(_ pause: ResourceTestPause, access: TeraCameraAccess) {
    pending = (pause, access)
  }

  func support() async -> TeraAddMediaSupport {
    guard let request = pending else { return .init(library: true, camera: true) }
    pending = nil
    await request.0.wait()
    return .init(library: true, camera: true, cameraAccess: request.1)
  }

  func importImages(limit _: Int) throws -> [TeraPreparedMedia] {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func captureImage() throws -> TeraPreparedMedia {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func open(_: [TeraPreparedMedia]) throws -> TeraOpenedMedia {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}
