import Foundation
@testable import TeraApp
import XCTest

extension TeraAddStoreTests {
  @MainActor
  func testStaleRevisionCaptureCannotEnterRuntimeAndRetryRetainsOriginalForm() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let preparation = TeraRevisionPreparation(client: client, media: nil)
    let target = TeraRevisionTarget(cardID: String(repeating: "a", count: 64),
                                    sourceEventID: String(repeating: "b", count: 64), sourceAddress: nil,
                                    authorPublicKey: String(repeating: "ab", count: 32))
    var form = TeraAddStore(runtimeClient: client).form
    form.content = "Captured before suspension"
    do {
      _ = try await preparation.prepare(target: target, form: form,
                                        identifier: { String(repeating: "c", count: 32) },
                                        ensureCurrent: { throw CancellationError() })
      XCTFail("Stale native generation must fail before admission")
    } catch is CancellationError {}
    let before = await backend.revisionPlanCount()
    XCTAssertEqual(before, 0)
    form.content = "Unrelated newer editing"
    let recovered = try await preparation.prepare(target: target, form: form,
                                                  identifier: { XCTFail("Must retain the original identity"); return "" },
                                                  ensureCurrent: {})
    XCTAssertEqual(recovered.replacement.form?.content, "Captured before suspension")
    XCTAssertEqual(recovered.operationID, String(repeating: "c", count: 32))
    _ = try await client.stop()
  }

  @MainActor
  func testRevisionLostReceiptRetainsCapturedRequestAndOneGraph() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    var original = store.form
    original.content = "Original form"
    let legacy = try await backend.saveAddIntent(input: TeraAddRuntimeInput(form: original, media: []),
                                                 existingDraftID: nil, expectedRevision: nil)
    await store.retractAndRevise(Self.card(localOperationID: legacy.id))
    store.updateForm(\.content, "Captured correction")
    await backend.failNextRevisionReceipt()
    await store.save()
    XCTAssertNil(store.activeDraft)
    XCTAssertTrue(store.revisionPreparation.isCaptured)
    XCTAssertFalse(store.isFormEditable)
    XCTAssertTrue(store.canSave)
    store.updateForm(\.content, "Must not replace the uncertain capture")
    XCTAssertEqual(store.form.content, "Captured correction")
    await store.save()
    let prepared = try XCTUnwrap(store.activeDraft)
    XCTAssertTrue(prepared.isRevision)
    XCTAssertEqual(prepared.form?.content, "Captured correction")
    XCTAssertFalse(store.revisionPreparation.isCaptured)
    await store.submit()
    XCTAssertEqual(store.activeDraft?.id, prepared.id)
    let count = await backend.revisionPlanCount()
    XCTAssertEqual(count, 1)
    _ = try await client.stop()
  }

  @MainActor
  func testRevisionOriginalFormUsesExactSourceKeyAndRejectsUnavailableOrWrongAuthor() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    var form = store.form
    form.content = "Retained source form"
    let source = try await backend.saveAddIntent(input: TeraAddRuntimeInput(form: form, media: []),
                                                 existingDraftID: nil, expectedRevision: nil)
    var card = Self.card(localOperationID: String(repeating: "f", count: 32))
    card.localSourceDraftID = source.id
    let editing = try await TeraAddCardIntents.revision(card, author: card.authorPublicKey, client: client)
    XCTAssertEqual(editing.form, form)
    for author in [nil, Optional(String(repeating: "c", count: 64))] {
      do {
        _ = try await TeraAddCardIntents.revision(card, author: author, client: client)
        XCTFail("Foreign or absent author must fail")
      } catch {}
    }
    card.localSourceDraftID = String(repeating: "d", count: 32)
    do {
      _ = try await TeraAddCardIntents.revision(card, author: card.authorPublicKey, client: client)
      XCTFail("Unavailable original must not start a revision")
    } catch {}
    let count = await backend.revisionPlanCount()
    XCTAssertEqual(count, 0)
    _ = try await client.stop()
  }
}
