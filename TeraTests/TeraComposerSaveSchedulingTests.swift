import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraComposerSaveSchedulingTests: XCTestCase {
  private let scope = TeraComposerScope(authorPublicKey: String(repeating: "a", count: 64), localNetworkID: "default")

  func testOrdinaryEditsCoalesceUntilDelayCompletes() async {
    let storage = ComposerTestStorage()
    let delay = ComposerCoalescingGate()
    let composer = make(storage, delay: delay)
    composer.change(form("first"))
    await delay.entered.wait()
    for index in 1 ... 1000 {
      composer.change(form("edit \(index)"))
    }
    let before = await storage.requests
    XCTAssertTrue(before.isEmpty)
    XCTAssertTrue(composer.isDirty)
    await delay.open()
    await TeraScopeFixtures.eventually { composer.state == .saved }
    let writes = await storage.requests
    XCTAssertEqual(writes.count, 1)
    XCTAssertEqual(writes.first?.form.content, "edit 1000")
    XCTAssertEqual(writes.first?.editSequence, 1001)
  }

  func testExplicitSaveCancelsOnlyPendingDelayAndWaitsForReceipt() async throws {
    let storage = ComposerTestStorage()
    let write = await storage.pauseNext()
    let delay = ComposerCoalescingGate()
    let composer = make(storage, delay: delay)
    composer.change(form("pending"))
    await delay.entered.wait()
    let save = Task { try await composer.save(form("newest")) }
    await write.entered.wait()
    let cancelled = await delay.cancelled
    XCTAssertTrue(cancelled)
    XCTAssertNil(composer.acknowledged)
    XCTAssertTrue(composer.isDirty)
    await write.resume.open()
    let receipt = try await save.value
    XCTAssertEqual(receipt.form.content, "newest")
    XCTAssertEqual(receipt.revision, 1)
    let writes = await storage.requests
    XCTAssertEqual(writes.count, 1)
  }

  func testExplicitSaveDuringActiveWriteDrainsNewestWithoutCancellingWriter() async throws {
    let storage = ComposerTestStorage()
    let write = await storage.pauseNext()
    let delay = ComposerCoalescingGate()
    let composer = make(storage, delay: delay)
    composer.change(form("first"))
    await delay.entered.wait()
    await delay.open()
    await write.entered.wait()
    let next = await storage.pauseNext()
    let save = Task { try await composer.save(form("second")) }
    await TeraScopeFixtures.eventually { composer.editSequence == 2 }
    await write.resume.open()
    await next.entered.wait()
    XCTAssertEqual(composer.acknowledged?.form.content, "first")
    XCTAssertTrue(composer.isDirty)
    await next.resume.open()
    let receipt = try await save.value
    XCTAssertEqual(receipt.form.content, "second")
    XCTAssertEqual(receipt.revision, 2)
    let cancelledWrites = await storage.cancelledWrites
    let waits = await delay.waitCount
    XCTAssertEqual(cancelledWrites, 0)
    XCTAssertEqual(waits, 1)
  }

  func testNoOpExplicitSaveDoesNotDisableNextAutosaveDelay() async throws {
    let storage = ComposerTestStorage()
    let delay = ComposerCoalescingGate()
    let composer = make(storage, delay: delay)
    let first = try await composer.save(form("saved"))
    let same = try await composer.save(form("saved"))
    XCTAssertEqual(first, same)
    composer.change(form("later edit"))
    await delay.entered.wait()
    let writes = await storage.requests
    XCTAssertEqual(writes.count, 1)
    XCTAssertEqual(composer.acknowledged, first)
    await delay.open()
    await TeraScopeFixtures.eventually { composer.state == .saved }
    XCTAssertEqual(composer.acknowledged?.form.content, "later edit")
  }

  func testStopCancelsDelayWithoutWritingAndResumeExplicitlySavesPendingEdit() async throws {
    let storage = ComposerTestStorage()
    let delay = ComposerCoalescingGate()
    let composer = make(storage, delay: delay)
    composer.change(form("pending"))
    await delay.entered.wait()
    composer.stop()
    XCTAssertEqual(composer.state, .unsaved)
    let writes = await storage.requests
    XCTAssertTrue(writes.isEmpty)
    XCTAssertNil(composer.acknowledged)
    composer.resume()
    let receipt = try await composer.save(form("pending"))
    XCTAssertEqual(receipt.form.content, "pending")
    XCTAssertEqual(receipt.revision, 1)
  }

  func testReplacingScopeDuringDelaySavesOnlyTheNewScope() async throws {
    let storage = ComposerTestStorage()
    let delay = ComposerCoalescingGate()
    let composer = make(storage, delay: delay)
    composer.change(form("old scope"))
    await delay.entered.wait()
    let next = TeraComposerScope(authorPublicKey: scope.authorPublicKey, localNetworkID: "next")
    composer.reset(scope: next)
    let receipt = try await composer.save(form("new scope"))
    XCTAssertEqual(receipt.scope, next)
    XCTAssertEqual(receipt.form.content, "new scope")
    let writes = await storage.requests
    XCTAssertEqual(writes.count, 1)
    XCTAssertEqual(writes.first?.scope, next)
  }

  func testExplicitFlushDoesNotHideANonCancellationFailureAtDelayBoundary() async {
    let storage = ComposerTestStorage()
    let pause = ResourceTestPause()
    let composer = TeraComposerAutosave(persistence: storage.port, delay: {
      await pause.wait()
      throw TeraRuntimeFailure.local(operation: "test.delay", code: "protected_data_unavailable",
                                     safeMessage: "Protected data unavailable.")
    })
    composer.reset(scope: scope)
    composer.change(form("pending"))
    await pause.entered.wait()
    let save = Task { try await composer.save(form("newest")) }
    await TeraScopeFixtures.eventually { composer.editSequence == 2 }
    await pause.resume.open()
    do {
      _ = try await save.value
      XCTFail("Only the coalescing cancellation may be bypassed.")
    } catch {
      XCTAssertEqual(TeraAddPresentation.failure(for: error)?.code, "protected_data_unavailable")
    }
    XCTAssertEqual(composer.state, .failed)
    XCTAssertTrue(composer.isDirty)
    XCTAssertNil(composer.acknowledged)
    let writes = await storage.requests
    XCTAssertTrue(writes.isEmpty)
  }

  func testSaveKeepsFlushingWhenAcknowledgmentStartsASuccessorWorker() async throws {
    let storage = ComposerTestStorage()
    let delay = ComposerCoalescingGate()
    let composer = make(storage, delay: delay)
    var addedSuccessor = false
    composer.stateChanged = { state in
      guard state == .saved, !addedSuccessor else { return }
      addedSuccessor = true
      composer.change(self.form("successor edit"))
    }
    defer { composer.stateChanged = { _ in } }
    let completed = expectation(description: "Explicit Save drains the successor without batching")
    let save = Task {
      defer { completed.fulfill() }
      return try await composer.save(form("first edit"))
    }
    await fulfillment(of: [completed], timeout: 2)
    // Release even on a failed expectation so a regression cannot strand work.
    await delay.open()
    let receipt = try await save.value
    XCTAssertTrue(addedSuccessor)
    XCTAssertEqual(receipt.form.content, "successor edit")
    XCTAssertEqual(receipt.revision, 2)
    XCTAssertEqual(composer.state, .saved)
    XCTAssertFalse(composer.isDirty)
    let writes = await storage.requests
    let cancelledWrites = await storage.cancelledWrites
    XCTAssertEqual(writes.count, 2)
    XCTAssertEqual(writes.first?.id, writes.last?.id)
    XCTAssertEqual(cancelledWrites, 0)
  }

  private func make(_ storage: ComposerTestStorage, delay: ComposerCoalescingGate) -> TeraComposerAutosave {
    let composer = TeraComposerAutosave(persistence: storage.port, delay: { try await delay.wait() })
    composer.reset(scope: scope)
    return composer
  }

  private func form(_ content: String) -> TeraComposerForm {
    var form = TeraComposerForm(commandType: .createUpdate)
    form.content = content
    return form
  }
}

actor ComposerCoalescingGate {
  let entered = ResourceTestGate()
  private var continuation: CheckedContinuation<Void, Error>?
  private var finished = false
  private(set) var cancelled = false
  private(set) var waitCount = 0

  func wait() async throws {
    waitCount += 1
    await entered.open()
    try await withTaskCancellationHandler {
      try Task.checkCancellation()
      guard !finished else { return }
      try await withCheckedThrowingContinuation { continuation = $0 }
    } onCancel: {
      Task { await self.cancel() }
    }
  }

  func open() {
    finished = true
    continuation?.resume()
    continuation = nil
  }

  private func cancel() {
    cancelled = true
    finished = true
    continuation?.resume(throwing: CancellationError())
    continuation = nil
  }
}
