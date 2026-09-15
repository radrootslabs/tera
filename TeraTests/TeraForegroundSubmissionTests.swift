import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraForegroundSubmissionTests: XCTestCase {
  func testViewCancellationRetainsForegroundResultAndFrozenCaptureWithoutNativeEnqueue() async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let media = AddMediaHarness(foreground: true)
    let store = TeraAddStore(runtimeClient: client, media: media)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)
    store.updateForm(\.content, "original foreground photo")
    await store.updateForm(\.media, [media.captureImage()])
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
    let settlements = await media.settlementValues()
    XCTAssertEqual(foreground, 1)
    XCTAssertEqual(native, 0)
    XCTAssertTrue(persisted)
    XCTAssertTrue(settlements.isEmpty)
    store.stop()
    _ = try await client.stop()
  }
}
