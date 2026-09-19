import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraForegroundSubmissionTests: XCTestCase {
  func testViewCancellationRetainsForegroundResultAndFrozenCaptureWithoutNativeEnqueue() async throws {
    try await exerciseForeground(useDefaultPolicy: false)
  }

  func testAdapterWithoutQualifiedNativePolicyUsesSharedForegroundAndFrozenCapture() async throws {
    try await exerciseForeground(useDefaultPolicy: true)
  }

  private func exerciseForeground(useDefaultPolicy: Bool) async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let harness = AddMediaHarness(foreground: true)
    let media: any TeraAddMediaHandling = if useDefaultPolicy {
      DefaultPolicyMedia(harness: harness)
    } else {
      harness
    }
    let store = TeraAddStore(runtimeClient: client, media: media)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)
    store.updateForm(\.content, "original foreground photo")
    await store.updateForm(\.media, [harness.captureImage()])
    let pause = ResourceTestPause()
    await backend.submissionBackend.pauseForeground(pause)
    let submit = Task { await store.submit() }
    await pause.entered.wait()
    let request = try XCTUnwrap(store.submissions.request)
    submit.cancel()
    store.updateForm(\.content, "newer editing")
    await store.submit()
    await pause.resume.open()
    await submit.value
    XCTAssertEqual(store.submissions.status?.request, request)
    XCTAssertEqual(store.submissions.status?.state, .complete)
    XCTAssertEqual(store.submissions.status?.captured.form.content, "original foreground photo")
    XCTAssertEqual(store.form.content, "newer editing")
    let foreground = await backend.submissionBackend.foregroundCount
    let native = await backend.submissionBackend.uploadCount
    let persisted = await backend.submissionBackend.completionPersisted
    let settlements = await harness.settlementValues()
    XCTAssertEqual(foreground, 1)
    XCTAssertEqual(native, 0)
    XCTAssertTrue(persisted)
    XCTAssertTrue(settlements.isEmpty)
    store.stop()
    _ = try await client.stop()
  }
}

/// Deliberately omits a native policy override; its media operations are usable.
private struct DefaultPolicyMedia: TeraAddMediaHandling {
  let harness: AddMediaHarness

  func confirmDurableComposerMedia(_ media: [TeraComposerMedia]) async throws {
    try await (harness as any TeraAddMediaHandling).confirmDurableComposerMedia(media)
  }

  func support() async throws -> TeraAddMediaSupport {
    await harness.support()
  }

  func importImages(limit: Int) async throws -> [TeraPreparedMedia] {
    await harness.importImages(limit: limit)
  }

  func captureImage() async throws -> TeraPreparedMedia {
    await harness.captureImage()
  }

  func open(_ media: [TeraPreparedMedia]) async throws -> TeraOpenedMedia {
    try await harness.open(media)
  }
}
