import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraSubmissionStoreTests: XCTestCase {
  func testTerminalOperationsReconcileReceiptsWithoutOpeningOrUploadingUnfinishedMedia() async throws {
    for state in [TeraOutboxState.complete, .terminal, .cancelled] {
      let backend = AddBackend()
      let client = try await TeraAddStoreTests.startedClient(backend)
      let store = await make(client, backend: backend)
      store.updateForm(\.content, "original")
      await store.submit()
      let original = try XCTUnwrap(store.submissions.status)
      let media = AddMediaHarness(failOpen: true)
      let item = await media.captureImage()
      var capturedForm = original.captured.form.editingValue
      capturedForm.media = [item]
      let captured = TeraComposerDraft(scope: original.captured.scope, id: original.captured.id,
                                       revision: original.captured.revision, editSequence: original.captured.editSequence,
                                       form: TeraComposerForm(editing: capturedForm))
      let status = TeraSubmissionStatus(
        request: original.request, intentID: original.intentID, operationID: original.operationID,
        revision: original.revision, captured: captured, state: state,
        committedAtUnixMilliseconds: original.committedAtUnixMilliseconds,
        updatedAtUnixMilliseconds: original.updatedAtUnixMilliseconds,
        media: [TeraSubmissionMedia(opaqueReference: item.opaqueReference, progress: TeraDraftMediaStatus(
          url: "https://blossom.example/\(item.sha256).png", stage: .pending, uploadAttempts: 0,
          verifiedAtUnixMilliseconds: nil, possibleOrphan: false, orphanReasonCode: nil, orphanRecordedAtUnixMilliseconds: nil
        ))], settlement: original.settlement
      )
      let before = await backend.submissionBackend.advanceCount
      let effects = TeraSubmissionEffects(client: client, media: media, ensure: {},
                                          accept: { _ in XCTFail("Terminal work cannot advance") })
      do { try await effects.advance(status) } catch { XCTFail("Terminal reconciliation must not open media: \(state)") }
      let opened = await media.openCount
      let reconciled = await media.submissionReconciliations
      let uploads = await backend.submissionBackend.uploadCount
      let after = await backend.submissionBackend.advanceCount
      XCTAssertEqual(opened, 0)
      XCTAssertEqual(uploads, 0)
      XCTAssertEqual(after, before)
      XCTAssertEqual(reconciled, 1)
      store.stop()
      _ = try await client.stop()
    }
  }

  func testDoubleTapAndTenThousandEditsKeepOneCapturedRequestDuringSlowPrepare() async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = await make(client, backend: backend)
    let pause = ResourceTestPause()
    await backend.submissionBackend.pausePrepare(pause)
    store.updateForm(\.content, "The tap's original text")
    let submit = Task { await store.submit() }
    await entered(pause)
    let original = try XCTUnwrap(store.submissions.request)
    XCTAssertTrue(store.submissions.isWorking)
    XCTAssertFalse(store.isWorking)
    XCTAssertTrue(store.isFormEditable)
    for index in 1 ... 10000 {
      store.updateForm(\.content, "later \(index)")
    }
    await store.submit()
    XCTAssertEqual(store.submissions.request, original)
    XCTAssertEqual(store.form.content, "later 10000")
    let captured = try await client.loadComposer(scope: original.scope, id: original.composerID)
    XCTAssertEqual(captured.form.content, "The tap's original text")
    XCTAssertEqual(captured.revision, original.expectedRevision)
    XCTAssertTrue(store.canSave)
    await store.save()
    XCTAssertEqual(store.savedComposer?.form.content, "later 10000")
    XCTAssertNotEqual(store.savedComposer?.id, original.composerID)
    XCTAssertEqual(store.composerState, .saved)
    // Navigation suspends observation without abandoning the active operation.
    store.suspend()
    await pause.resume.open()
    await submit.value
    await store.start()
    await store.save()
    XCTAssertEqual(store.submissions.status?.state, .complete)
    XCTAssertEqual(store.submissions.status?.captured.form.content, "The tap's original text")
    XCTAssertEqual(store.savedComposer?.form.content, "later 10000")
    XCTAssertEqual(store.form.content, "later 10000")
    let counts = await (backend.submissionBackend.idCount, backend.submissionBackend.prepareCount)
    XCTAssertEqual(counts.0, 1)
    XCTAssertEqual(counts.1, 1)
    _ = try await client.stop()
  }

  func testCancellingViewWaiterDoesNotCancelOrReplaceTheOperation() async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = await make(client, backend: backend)
    let pause = ResourceTestPause()
    await backend.submissionBackend.pauseAdvance(pause)
    store.updateForm(\.content, "original")
    let submit = Task { await store.submit() }
    await entered(pause)
    submit.cancel()
    store.updateForm(\.content, "newer")
    await store.submit()
    XCTAssertTrue(store.submissions.isWorking)
    await pause.resume.open()
    await submit.value
    XCTAssertEqual(store.submissions.status?.state, .complete)
    XCTAssertEqual(store.submissions.status?.captured.form.content, "original")
    XCTAssertEqual(store.form.content, "newer")
    let count = await backend.submissionBackend.prepareCount
    XCTAssertEqual(count, 1)
    _ = try await client.stop()
  }

  func testExplicitStopWaitKeepsWorkerSlotAndRecoversLateCommitWithOriginalID() async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = await make(client, backend: backend)
    let pause = ResourceTestPause()
    await backend.submissionBackend.pausePrepare(pause)
    store.updateForm(\.content, "original")
    let submit = Task { await store.submit() }
    await entered(pause)
    let request = try XCTUnwrap(store.submissions.request)
    store.submissions.stopWaiting()
    XCTAssertTrue(store.submissions.isWorking)
    await store.submit()
    store.updateForm(\.content, "later")
    await pause.resume.open()
    await submit.value
    XCTAssertNil(store.submissions.status)
    XCTAssertEqual(store.submissions.request, request)
    XCTAssertFalse(store.submissions.isWorking)
    await store.submit()
    XCTAssertEqual(store.submissions.request, request)
    XCTAssertEqual(store.submissions.status?.state, .complete)
    XCTAssertEqual(store.submissions.status?.captured.form.content, "original")
    XCTAssertEqual(store.form.content, "later")
    let count = await backend.submissionBackend.prepareCount
    XCTAssertEqual(count, 1)
    _ = try await client.stop()
  }

  func testLostCommitAndUnreadableRecoveryCannotTurnNewerEditingIntoAReplacementRequest() async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = await make(client, backend: backend)
    let pause = ResourceTestPause()
    await backend.submissionBackend.pausePrepare(pause, loseReceipt: true)
    store.updateForm(\.content, "original")
    let submit = Task { await store.submit() }
    await entered(pause)
    let request = try XCTUnwrap(store.submissions.request)
    await backend.submissionBackend.unreadable(true)
    store.updateForm(\.content, "newer editing")
    await pause.resume.open()
    await submit.value
    await store.submit()
    XCTAssertEqual(store.submissions.request, request)
    XCTAssertNil(store.submissions.status)
    let captured = try await client.loadComposer(scope: request.scope, id: request.composerID)
    XCTAssertEqual(captured.form.content, "original")
    XCTAssertEqual(captured.revision, request.expectedRevision)
    await store.save()
    XCTAssertEqual(store.savedComposer?.form.content, "newer editing")
    XCTAssertNotEqual(store.savedComposer?.id, request.composerID)
    XCTAssertEqual(store.form.content, "newer editing")
    await backend.submissionBackend.unreadable(false)
    await store.submit()
    XCTAssertEqual(store.submissions.status?.request, request)
    XCTAssertEqual(store.submissions.status?.captured.form.content, "original")
    let count = await backend.submissionBackend.prepareCount
    XCTAssertEqual(count, 1)
    _ = try await client.stop()
  }

  func testRecreatedStoreSelectsSavedOperationWithoutChangingEditingOrBeginningEffects() async throws {
    let backend = AddBackend(advanceOffline: true)
    let client = try await TeraAddStoreTests.startedClient(backend)
    let first = await make(client, backend: backend)
    first.updateForm(\.content, "original")
    await first.submit()
    let saved = try XCTUnwrap(first.submissions.status)
    first.stop()
    let replacement = await make(client, backend: backend)
    replacement.updateForm(\.content, "reopened editing")
    await TeraScopeFixtures.eventually { !replacement.submissions.inventory.isLoading }
    guard case let .submission(summary) = replacement.submissions.inventory.entries.first else {
      return XCTFail("Original operation must be selectable after relaunch")
    }
    let before = await backend.submissionBackend.advanceCount
    await replacement.submissions.select(summary)
    let after = await backend.submissionBackend.advanceCount
    XCTAssertEqual(after, before)
    XCTAssertEqual(replacement.submissions.status, saved)
    XCTAssertEqual(replacement.form.content, "reopened editing")
    await replacement.submit()
    XCTAssertEqual(replacement.submissions.status?.operationID, saved.operationID)
    _ = try await client.stop()
  }

  func testExplicitNewMakesDistinctOperationForIdenticalIntentionalContent() async throws {
    let backend = AddBackend()
    let client = try await TeraAddStoreTests.startedClient(backend)
    let store = await make(client, backend: backend)
    store.updateForm(\.content, "identical")
    await store.submit()
    let first = try XCTUnwrap(store.submissions.status)
    await store.submit()
    XCTAssertEqual(store.submissions.status, first)
    store.newDraft()
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    store.updateForm(\.content, "identical")
    await store.submit()
    let second = try XCTUnwrap(store.submissions.status)
    XCTAssertNotEqual(second.request.commandID, first.request.commandID)
    XCTAssertNotEqual(second.operationID, first.operationID)
    XCTAssertEqual(second.captured.form, first.captured.form)
    let count = await backend.submissionBackend.prepareCount
    XCTAssertEqual(count, 2)
    _ = try await client.stop()
  }

  private func make(_ client: TeraRuntimeClient, backend: AddBackend) async -> TeraAddStore {
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    return store
  }

  private func entered(_ pause: ResourceTestPause) async {
    let reached = expectation(description: "Submission reached controlled native boundary")
    let observer = Task { await pause.entered.wait(); reached.fulfill() }
    await fulfillment(of: [reached], timeout: 3)
    await pause.entered.open()
    await observer.value
  }
}
