import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraComposerSubmissionCaptureTests: XCTestCase {
  private let scope = TeraComposerScope(authorPublicKey: String(repeating: "a", count: 64), localNetworkID: "default")

  func testTapCapturesOneRevisionWhileTenThousandLaterEditsStayUnsaved() async throws {
    let storage = ComposerTestStorage()
    let first = await storage.pauseNext()
    let composer = make(storage)
    composer.change(form("before the tap"))
    await first.entered.wait()
    let second = await storage.pauseNext()
    let capture = try composer.beginSubmissionCapture(form("captured at the tap"))
    let save = Task { try await composer.saveSubmissionCapture(capture) }
    for index in 1 ... 10000 {
      composer.change(form("later \(index)"))
    }
    await first.resume.open()
    await second.entered.wait()
    XCTAssertEqual(composer.acknowledged?.form.content, "before the tap")
    await second.resume.open()
    let captured = try await save.value
    XCTAssertEqual(captured.form.content, "captured at the tap")
    XCTAssertEqual(captured.editSequence, 2)
    XCTAssertEqual(captured.revision, 2)
    XCTAssertTrue(composer.isDirty)
    XCTAssertEqual(composer.state, .unsaved)
    let writes = await storage.requests
    XCTAssertEqual(writes.map(\.form.content), ["before the tap", "captured at the tap"])
    XCTAssertThrowsError(try composer.beginSubmissionCapture(form("another tap")))
    let replay = try await composer.saveSubmissionCapture(capture)
    XCTAssertEqual(replay, captured)
    let repeated = await storage.requests
    XCTAssertEqual(repeated, writes)
    composer.releaseSubmissionCapture(capture)
    let latest = try await composer.save(form("later 10000"))
    XCTAssertEqual(latest.form.content, "later 10000")
    XCTAssertEqual(latest.revision, 3)
    XCTAssertEqual(latest.id, captured.id)
    XCTAssertEqual(latest.editSequence, 10002)
  }

  func testCaptureCancelsBatchingWithoutLettingTheNewestEditReplaceItsSource() async throws {
    let storage = ComposerTestStorage()
    let delay = ComposerCoalescingGate()
    let composer = TeraComposerAutosave(persistence: storage.port, delay: { try await delay.wait() })
    composer.reset(scope: scope)
    composer.change(form("batched"))
    await delay.entered.wait()
    let capture = try composer.beginSubmissionCapture(form("tap"))
    composer.change(form("typed after tap"))
    let saved = try await composer.saveSubmissionCapture(capture)
    XCTAssertEqual(saved.form.content, "tap")
    XCTAssertEqual(saved.editSequence, 2)
    let writes = await storage.requests
    let cancelled = await delay.cancelled
    XCTAssertEqual(writes.count, 1)
    XCTAssertTrue(cancelled)
    XCTAssertTrue(composer.isDirty)
    composer.releaseSubmissionCapture(capture)
    let latest = try await composer.save(form("typed after tap"))
    XCTAssertEqual(latest.revision, 2)
  }

  func testUnknownCaptureWriteReconcilesTheOriginalBeforeSavingNewerEditing() async throws {
    let storage = ComposerTestStorage()
    await storage.failNext(.afterCommit)
    let composer = make(storage)
    let capture = try composer.beginSubmissionCapture(form("original"))
    do {
      _ = try await composer.saveSubmissionCapture(capture)
      XCTFail("A lost receipt cannot claim the capture was saved.")
    } catch { XCTAssertNil(composer.acknowledged) }
    composer.change(form("newest"))
    do {
      _ = try await composer.save(form("newest"))
      XCTFail("Explicit Save cannot overwrite the source while submission is unresolved.")
    } catch {
      XCTAssertEqual(TeraAddPresentation.failure(for: error)?.code, "submission_capture_pending")
    }
    let recovered = try await composer.saveSubmissionCapture(capture)
    XCTAssertEqual(recovered.form.content, "original")
    XCTAssertEqual(recovered.revision, 1)
    let writes = await storage.requests
    let reads = await storage.readCount
    XCTAssertEqual(writes.count, 1)
    XCTAssertEqual(reads, 1)
    composer.releaseSubmissionCapture(capture)
    let saved = try await composer.save(form("newest"))
    XCTAssertEqual(saved.revision, 2)
    XCTAssertEqual(saved.form.content, "newest")
  }

  func testStopRetainsTheBarrierAndResumeReconcilesTheLateWrite() async throws {
    let storage = ComposerTestStorage()
    let pause = await storage.pauseNext()
    let composer = make(storage)
    let capture = try composer.beginSubmissionCapture(form("captured"))
    let save = Task { try await composer.saveSubmissionCapture(capture) }
    await pause.entered.wait()
    composer.change(form("newer"))
    composer.stop()
    await pause.resume.open()
    do { _ = try await save.value; XCTFail("A stale waiter cannot acknowledge the write.") } catch {}
    XCTAssertTrue(composer.hasSubmissionCapture)
    XCTAssertNil(composer.acknowledged)
    composer.resume()
    let recovered = try await composer.saveSubmissionCapture(capture)
    XCTAssertEqual(recovered.form.content, "captured")
    XCTAssertEqual(recovered.revision, 1)
    let writes = await storage.requests
    XCTAssertEqual(writes.count, 1)
    composer.releaseSubmissionCapture(capture)
    _ = try await composer.save(form("newer"))
  }

  func testOldCaptureCannotReleaseAReplacementScopeBarrier() async throws {
    let storage = ComposerTestStorage()
    let pause = await storage.pauseNext()
    let composer = make(storage)
    let original = try composer.beginSubmissionCapture(form("old"))
    let save = Task { try await composer.saveSubmissionCapture(original) }
    await pause.entered.wait()
    let nextScope = TeraComposerScope(authorPublicKey: scope.authorPublicKey, localNetworkID: "next")
    composer.reset(scope: nextScope)
    let replacement = try composer.beginSubmissionCapture(form("new scope"))
    composer.releaseSubmissionCapture(original)
    XCTAssertTrue(composer.hasSubmissionCapture)
    await pause.resume.open()
    do { _ = try await save.value; XCTFail("Replaced scope cannot accept an old capture.") } catch {}
    let current = try await composer.saveSubmissionCapture(replacement)
    XCTAssertEqual(current.scope, nextScope)
    XCTAssertEqual(current.form.content, "new scope")
    composer.releaseSubmissionCapture(replacement)
    XCTAssertFalse(composer.hasSubmissionCapture)
  }

  private func make(_ storage: ComposerTestStorage) -> TeraComposerAutosave {
    let composer = TeraComposerAutosave(persistence: storage.port, delay: {})
    composer.reset(scope: scope)
    return composer
  }

  private func form(_ text: String) -> TeraComposerForm {
    var value = TeraComposerForm(commandType: .createUpdate)
    value.content = text
    return value
  }
}
