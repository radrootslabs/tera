import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraCoordinateAdmissionTests: XCTestCase {
  func testHeldLegacyRequestRetainsEditingAndCannotResumeOrPublish() async throws {
    let backend = try TeraScopeBackend()
    var held = TeraScopeFixtures.draft("Retained captured form")
    held.coordinateWritable = false
    held.coordinateCaptured = true
    await backend.setDrafts([held])
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    store.updateForm(\.content, "Current editing must survive")
    await store.save()
    let composer = try XCTUnwrap(store.savedComposer)
    await store.retry(id: held.id)
    XCTAssertEqual(store.form.content, "Current editing must survive")
    XCTAssertEqual(store.savedComposer, composer)
    XCTAssertEqual(store.message, TeraPublicationActionReason.coordinateChanged.explanation)
    XCTAssertFalse(held.canAdvance)
    XCTAssertFalse(held.canQueue)
    XCTAssertFalse(held.isEditable)
    var accepted: TeraDraftStatus?
    let submission = TeraLegacySubmission(
      runtimeClient: client, media: nil, revisionID: { nil }, initial: { held }, ensure: {},
      acceptDraft: { accepted = $0 }, acceptRevision: { _ in XCTFail("Held request cannot revise") },
      message: { XCTAssertEqual($0, held.honestSummary) }, refreshMedia: {}
    )
    try await submission.submit()
    XCTAssertEqual(accepted, held)
    store.stop()
    _ = try await client.stop()
  }

  func testCapturedCoordinateFreezesFormWhilePreservingTheSameRequestForContinuation() {
    var captured = TeraScopeFixtures.draft("Captured form")
    captured.coordinateCaptured = true
    XCTAssertTrue(captured.coordinateWritable)
    XCTAssertFalse(captured.isEditable)
    XCTAssertTrue(captured.canQueue)
    let summary = TeraLegacyDraftSummary(
      id: captured.id, revision: captured.revision, kind: captured.kind,
      commandType: captured.commandType, state: captured.state, hasForm: true, isRevision: false,
      createdAtUnixMilliseconds: captured.createdAtUnixMilliseconds,
      updatedAtUnixMilliseconds: captured.updatedAtUnixMilliseconds, mediaCount: 0,
      verifiedMediaCount: 0, possibleOrphanCount: 0, settlement: captured.settlement,
      coordinateWritable: false, coordinateCaptured: true
    )
    XCTAssertFalse(summary.isEditable)
    XCTAssertFalse(summary.canAdvance)
    XCTAssertFalse(summary.canQueue)
    XCTAssertEqual(summary.honestSummary, TeraPublicationActionReason.coordinateChanged.explanation)
    XCTAssertEqual(summary.id, captured.id)
  }
}
