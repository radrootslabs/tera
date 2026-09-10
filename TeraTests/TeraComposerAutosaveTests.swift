import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraComposerAutosaveTests: XCTestCase {
  private let scope = TeraComposerScope(authorPublicKey: String(repeating: "a", count: 64), localNetworkID: "default")

  func testSlowWriteCoalescesTenThousandEditsAndExplicitSaveAwaitsNewestReceipt() async throws {
    let storage = ComposerTestStorage()
    let first = await storage.pauseNext()
    let composer = make(storage)
    composer.change(form("first"))
    await first.entered.wait()
    let second = await storage.pauseNext()
    for number in 1 ... 10000 {
      composer.change(form("edit \(number)"))
    }
    let save = Task { try await composer.save(form("edit 10000")) }
    let writesBefore = await storage.requests.count
    XCTAssertEqual(writesBefore, 1)
    XCTAssertNil(composer.acknowledged)
    XCTAssertTrue(composer.isDirty)
    await first.resume.open()
    await second.entered.wait()
    XCTAssertEqual(composer.acknowledged?.form.content, "first")
    XCTAssertTrue(composer.isDirty)
    XCTAssertEqual(composer.state, .saving)
    let requests = await storage.requests
    XCTAssertEqual(requests.map(\.editSequence), [1, 10001])
    XCTAssertEqual(requests.last?.expectedRevision, 1)
    await second.resume.open()
    let receipt = try await save.value
    XCTAssertEqual(receipt.form.content, "edit 10000")
    XCTAssertEqual(receipt.editSequence, 10001)
    XCTAssertEqual(composer.state, .saved)
    XCTAssertFalse(composer.isDirty)
  }

  func testReplacedScopesKeepOneWorkerAndDiscardLateOldReceipts() async throws {
    let storage = ComposerTestStorage()
    let old = await storage.pauseNext()
    let composer = make(storage)
    composer.change(form("old account"))
    await old.entered.wait()
    for number in 1 ... 100 {
      composer.reset(scope: TeraComposerScope(authorPublicKey: scope.authorPublicKey, localNetworkID: "context-\(number)"))
      composer.change(form("new \(number)"))
    }
    let next = await storage.pauseNext()
    let writesBefore = await storage.requests.count
    XCTAssertEqual(writesBefore, 1)
    let save = Task { try await composer.save(form("new 100")) }
    await old.resume.open()
    await next.entered.wait()
    XCTAssertNil(composer.acknowledged)
    XCTAssertTrue(composer.isDirty)
    await next.resume.open()
    let receipt = try await save.value
    XCTAssertEqual(receipt.scope.localNetworkID, "context-100")
    XCTAssertEqual(receipt.form.content, "new 100")
    let requests = await storage.requests
    XCTAssertEqual(requests.count, 2)
    XCTAssertNotEqual(requests.first?.id, requests.last?.id)
  }

  func testLostCommitCallbackRemainsFailedUntilExactReadReconcilesWithoutDuplicateWrite() async throws {
    let storage = ComposerTestStorage()
    await storage.failNext(.afterCommit)
    let composer = make(storage)
    await assertSaveFails(composer, form: form("persisted but unacknowledged"))
    XCTAssertEqual(composer.state, .failed)
    XCTAssertNil(composer.acknowledged)
    let receipt = try await composer.save(form("persisted but unacknowledged"))
    XCTAssertEqual(receipt.revision, 1)
    XCTAssertEqual(composer.state, .saved)
    let requests = await storage.requests
    let reads = await storage.readCount
    XCTAssertEqual(requests.count, 1)
    XCTAssertEqual(reads, 1)
  }

  func testFailedWriteRetainsNewestEditAndRetriesSameIDAfterNotFound() async throws {
    let storage = ComposerTestStorage()
    await storage.failNext(.beforeCommit)
    let composer = make(storage)
    await assertSaveFails(composer, form: form("first"))
    composer.change(form("newest"))
    XCTAssertEqual(composer.state, .failed)
    let receipt = try await composer.save(form("newest"))
    XCTAssertEqual(receipt.form.content, "newest")
    XCTAssertEqual(receipt.editSequence, 2)
    let requests = await storage.requests
    XCTAssertEqual(requests.count, 2)
    XCTAssertEqual(requests.first?.id, requests.last?.id)
    XCTAssertNil(requests.last?.expectedRevision)
  }

  func testWrongReceiptAndConcurrentWriterNeverBecomeSavedOrOverwriteTheirRevision() async {
    let storage = ComposerTestStorage()
    await storage.failNext(.differentWriter)
    let composer = make(storage)
    await assertSaveFails(composer, form: form("my edit"))
    await assertSaveFails(composer, form: form("keep my newer edit"))
    XCTAssertNil(composer.acknowledged)
    XCTAssertTrue(composer.isDirty)
    XCTAssertEqual(composer.state, .failed)
    let requests = await storage.requests
    XCTAssertEqual(requests.count, 1)
  }

  func testStopDoesNotClaimLateWriteSavedAndResumeReconcilesItsExactIdentity() async throws {
    let storage = ComposerTestStorage()
    let paused = await storage.pauseNext()
    let composer = make(storage)
    composer.change(form("background"))
    await paused.entered.wait()
    composer.stop()
    await paused.resume.open()
    await storage.completed.wait()
    XCTAssertNil(composer.acknowledged)
    XCTAssertEqual(composer.state, .unsaved)
    composer.resume()
    let receipt = try await composer.save(form("background"))
    XCTAssertEqual(receipt.revision, 1)
    let requests = await storage.requests
    XCTAssertEqual(requests.count, 1)
    XCTAssertEqual(composer.state, .saved)
  }

  func testNoOpEditsDoNotWriteAndUnavailableScopeCannotClaimSaved() async throws {
    let storage = ComposerTestStorage()
    let composer = make(storage)
    let first = try await composer.save(form("same"))
    for _ in 1 ... 1000 {
      composer.change(form("same"))
    }
    let repeated = try await composer.save(form("same"))
    XCTAssertEqual(first, repeated)
    let requests = await storage.requests
    XCTAssertEqual(requests.count, 1)
    composer.reset(scope: nil)
    await assertSaveFails(composer, form: form("not scoped"))
    XCTAssertEqual(composer.state, .failed)
    XCTAssertTrue(composer.isDirty)
  }

  func testStoreKeepsNewerTextAndSaveWaitsForTheLatestCoalescedReceipt() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    let first = await backend.pause(.composer)
    store.updateForm(\.content, "first")
    let save = Task { await store.save() }
    await first.entered.wait()
    let second = await backend.pause(.composer)
    store.updateForm(\.content, "newest")
    await first.resume.open()
    await second.entered.wait()
    XCTAssertEqual(store.form.content, "newest")
    XCTAssertEqual(store.savedComposer?.form.content, "first")
    XCTAssertNil(store.message)
    XCTAssertTrue(store.isWorking)
    await second.resume.open()
    await save.value
    XCTAssertEqual(store.savedComposer?.form.content, "newest")
    XCTAssertEqual(store.form.content, "newest")
    XCTAssertEqual(store.composerState, .saved)
    XCTAssertFalse(store.isWorking)
    store.stop()
    _ = try await client.stop()
  }

  func testStoreDiscardsOldAccountReceiptAndPreservesEditingAcrossRelayReconfiguration() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.start()
    let first = await backend.pause(.composer)
    store.updateForm(\.content, "old account")
    let old = Task { await store.save() }
    await first.entered.wait()
    let updated = TeraScopeFixtures.snapshot(account: "b")
    await backend.configure(updated)
    store.configure(snapshot: updated)
    await store.start()
    store.updateForm(\.content, "new account")
    let current = Task { await store.save() }
    await first.resume.open()
    await old.value
    await current.value
    let receipt = try XCTUnwrap(store.savedComposer)
    XCTAssertEqual(receipt.scope.authorPublicKey, String(repeating: "b", count: 64))
    XCTAssertEqual(receipt.form.content, "new account")
    store.configure(snapshot: TeraScopeFixtures.snapshot(account: "b", relay: "second", profile: "new profile"))
    XCTAssertEqual(store.form.content, "new account")
    await store.start()
    await store.save()
    XCTAssertEqual(store.savedComposer, receipt)
    store.stop()
    _ = try await client.stop()
  }

  private func make(_ storage: ComposerTestStorage) -> TeraComposerAutosave {
    let composer = TeraComposerAutosave(persistence: storage.port, delay: {})
    composer.reset(scope: scope)
    return composer
  }

  private func form(_ content: String) -> TeraComposerForm {
    var form = TeraComposerForm(commandType: .createEvent)
    form.content = content
    form.eventStartDate = "2026-09-"
    form.priceAmount = "12."
    return form
  }

  private func assertSaveFails(_ composer: TeraComposerAutosave, form: TeraComposerForm) async {
    do {
      _ = try await composer.save(form)
      XCTFail("Unconfirmed changes must not receive a saved acknowledgment.")
    } catch { XCTAssertTrue(composer.isDirty) }
  }
}

actor ComposerTestStorage {
  enum Failure { case beforeCommit, afterCommit, differentWriter }
  private var reserved = 0
  private var values: [String: TeraComposerDraft] = [:]
  private var pause: ResourceTestPause?
  private var failure: Failure?
  private(set) var requests: [TeraComposerSaveRequest] = []
  private(set) var readCount = 0
  private(set) var cancelledWrites = 0
  let completed = ResourceTestGate()

  nonisolated var port: TeraComposerPersistence {
    TeraComposerPersistence(reserve: { await self.reserve() }, save: { try await self.save($0) },
                            load: { try await self.load($0, id: $1) })
  }

  func pauseNext() -> ResourceTestPause {
    let value = ResourceTestPause()
    pause = value
    return value
  }

  func failNext(_ failure: Failure) {
    self.failure = failure
  }

  func reserve() -> String {
    reserved += 1
    return String(format: "%032x", reserved)
  }

  func save(_ request: TeraComposerSaveRequest) async throws -> TeraComposerSaveReceipt {
    requests.append(request)
    let pause = pause
    self.pause = nil
    let failure = failure
    self.failure = nil
    await pause?.wait()
    if Task.isCancelled {
      cancelledWrites += 1
    }
    if failure == .beforeCommit {
      throw TeraComposerAcknowledgment.unconfirmed
    }
    guard values[request.id]?.revision == request.expectedRevision else { throw TeraComposerAcknowledgment.unconfirmed }
    var form = request.form
    if failure == .differentWriter {
      form.content = "a concurrent writer"
    }
    let draft = TeraComposerDraft(scope: request.scope, id: request.id, revision: (request.expectedRevision ?? 0) + 1,
                                  editSequence: request.editSequence, form: form)
    values[request.id] = draft
    await completed.open()
    if failure == .afterCommit {
      throw TeraComposerAcknowledgment.unconfirmed
    }
    return TeraComposerSaveReceipt(draft: draft, replayed: false)
  }

  func load(_ scope: TeraComposerScope, id: String) throws -> TeraComposerDraft {
    readCount += 1
    guard let draft = values[id], draft.scope == scope else {
      throw TeraRuntimeFailure.local(operation: "test.composer", code: "composer_not_found", safeMessage: "Not saved.")
    }
    return draft
  }
}
