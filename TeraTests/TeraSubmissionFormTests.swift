import Foundation
@testable import TeraApp
import XCTest

extension TeraAddStoreTests {
  @MainActor
  func testAllFiveFormsCompleteWithFrozenSubmissionsAndContinuedEditing() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(
      runtimeClient: client,
      media: AddMediaHarness()
    )
    await store.configure(snapshot: backend.snapshot())
    await store.start()

    for type in TeraAddCommandType.allCases {
      store.newDraft(type: type)
      await TeraScopeFixtures.eventually { !store.protection.isWorking }
      configure(store, type: type)
      if type == .createPhotoUpdate {
        await store.importPhotos()
        XCTAssertEqual(store.form.media.count, 1)
        XCTAssertNil(store.form.media.first?.remoteURL)
      }
      await store.submit()
      XCTAssertEqual(store.submissions.status?.captured.form.commandType, type)
      XCTAssertEqual(store.submissions.status?.state, .complete)
      XCTAssertEqual(store.submissions.status?.captured.form.editingValue, store.form)
      if type == .createPhotoUpdate {
        XCTAssertEqual(
          store.submissions.status?.preparedMedia.first?.remoteURL,
          "http://127.0.0.1:3000/\(String(repeating: "0", count: 64)).png"
        )
        let didUpload = await backend.submissionBackend.uploadCount > 0
        XCTAssertTrue(didUpload)
      }
    }

    let frozen = try XCTUnwrap(store.submissions.status)
    store.updateForm(\.content, "mutated after submit")
    XCTAssertEqual(store.form.content, "mutated after submit")
    XCTAssertEqual(store.submissions.status, frozen)
    XCTAssertTrue(store.isFormEditable)
    XCTAssertNil(store.activeDraft)
    XCTAssertTrue(store.drafts.isEmpty)
    let count = await backend.submissionBackend.prepareCount
    XCTAssertEqual(count, 5)
    _ = try await client.stop()
  }
}
