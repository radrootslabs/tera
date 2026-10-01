import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraPublicationNavigationTests: XCTestCase {
  func testNewDuringUploadAndAdvanceRetainsWorkerAndCreatesSeparateComposer() async throws {
    for upload in [false, true] {
      let fixture = try await NavigationFixture(upload: upload)
      let store = fixture.store
      let original = try XCTUnwrap(store.submissions.status)
      XCTAssertTrue(store.canCreateNewComposer)
      // Navigation cancels the view waiter and observation, not publication.
      fixture.waiter.cancel()
      store.suspend()
      await store.start()
      store.newDraft(type: .createAsk)
      await TeraScopeFixtures.eventually { !store.protection.isWorking }
      XCTAssertFalse(store.protection.failed)
      XCTAssertEqual(store.form.commandType, .createAsk)
      XCTAssertTrue(store.form.content.isEmpty)
      XCTAssertTrue(store.submissions.isWorking)
      XCTAssertEqual(store.submissions.request, original.request)
      XCTAssertEqual(store.submissions.status?.captured, original.captured)
      XCTAssertEqual(store.submitLabel, "Submit")
      XCTAssertFalse(store.canSubmit, "One bounded worker; New does not start another effect")
      store.updateForm(\.content, "separate new editing")
      await store.save()
      let editing = try XCTUnwrap(store.savedComposer)
      XCTAssertNotEqual(editing.id, original.captured.id)
      XCTAssertEqual(editing.form.content, "separate new editing")
      await fixture.pause.resume.open()
      await fixture.waiter.value
      XCTAssertEqual(store.submissions.status?.request, original.request)
      XCTAssertEqual(store.submissions.status?.captured, original.captured)
      XCTAssertEqual(store.form.content, "separate new editing")
      XCTAssertTrue(store.canSubmit)
      await store.submit()
      let next = try XCTUnwrap(store.submissions.status)
      XCTAssertNotEqual(next.request.commandID, original.request.commandID)
      XCTAssertNotEqual(next.captured.id, original.captured.id)
      XCTAssertEqual(next.captured.form.content, "separate new editing")
      let count = await fixture.backend.submissionBackend.prepareCount
      XCTAssertEqual(count, 2, "Exactly one original and one explicit new submission")
      store.stop()
      _ = try await fixture.client.stop()
    }
  }

  func testNewRefusesUnacknowledgedPreparationWithoutDiscardingRequest() async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "pending capture")
    let pause = ResourceTestPause()
    await backend.submissionBackend.pausePrepare(pause)
    let waiter = Task { await store.submit() }
    await pause.entered.wait()
    let request = store.submissions.request
    XCTAssertFalse(store.canCreateNewComposer)
    store.newDraft()
    XCTAssertEqual(store.form.content, "pending capture")
    XCTAssertEqual(store.submissions.request, request)
    await pause.resume.open()
    await waiter.value
    store.stop()
    _ = try await client.stop()
  }

  func testExplicitStopAfterNewStillTargetsOriginalPublication() async throws {
    let fixture = try await NavigationFixture(upload: false)
    let store = fixture.store
    let request = try XCTUnwrap(store.submissions.request)
    store.newDraft(type: .createAsk)
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    store.updateForm(\.content, "retain separate editing")
    await store.submissions.requestStop()
    XCTAssertEqual(store.submissions.request, request)
    XCTAssertEqual(store.submissions.status?.delivery.isStopped, true)
    await fixture.pause.resume.open()
    await fixture.waiter.value
    XCTAssertEqual(store.form.content, "retain separate editing")
    XCTAssertEqual(store.submissions.request, request)
    let count = await fixture.backend.submissionBackend.prepareCount
    XCTAssertEqual(count, 1)
    store.stop()
    _ = try await fixture.client.stop()
  }
}

@MainActor
private struct NavigationFixture {
  let backend: AddBackend
  let client: TeraRuntimeClient
  let store: TeraAddStore
  let pause: ResourceTestPause
  let waiter: Task<Void, Never>

  init(upload: Bool) async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let media = AddMediaHarness(foreground: true)
    let store = TeraAddStore(runtimeClient: client, media: media)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    if upload {
      store.selectType(.createPhotoUpdate)
      await TeraScopeFixtures.eventually { !store.protection.isWorking }
      let photo = await media.captureImage()
      store.updateForm(\.media, [photo])
    }
    store.updateForm(\.content, "frozen original")
    let pause = ResourceTestPause()
    if upload {
      await backend.submissionBackend.pauseForeground(pause)
    } else {
      await backend.submissionBackend.pauseAdvance(pause)
    }
    let waiter = Task { await store.submit() }
    await pause.entered.wait()
    self.backend = backend
    self.client = client
    self.store = store
    self.pause = pause
    self.waiter = waiter
  }
}
