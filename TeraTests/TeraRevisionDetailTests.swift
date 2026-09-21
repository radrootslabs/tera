import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraRevisionDetailTests: XCTestCase {
  func testActionsReloadCurrentPermissionAndPreservePartialStoppedEvidence() async throws {
    let harness = RevisionDetailHarness(value: Self.fixture())
    let store = TeraRevisionDetailStore(operations: harness.operations)
    store.configure(author: Self.author)
    await store.load(Self.id)
    XCTAssertTrue(try XCTUnwrap(store.status).canResume)
    var stopped = Self.fixture()
    stopped.canResume = false
    stopped.canCancel = false
    stopped.replacementProgress = TeraRevisionBranchStatus(stopped: true, canResume: false, canCancel: false,
                                                           targets: stopped.replacementProgress.targets)
    await harness.set(stopped)
    await store.resume()
    await store.cancel()
    let counts = await harness.counts()
    XCTAssertEqual(counts, [0, 0])
    XCTAssertEqual(store.status, stopped)
    XCTAssertEqual(store.status?.honestSummary, "Revision has partial or uncertain relay outcomes")
    XCTAssertEqual(store.status?.replacementProgress.targets?.targets.first?.accepted, true)
  }

  func testExplicitActionsRetainRelationAndUseOneCurrentOperation() async {
    let harness = RevisionDetailHarness(value: Self.fixture())
    let store = TeraRevisionDetailStore(operations: harness.operations)
    store.configure(author: Self.author)
    await store.load(Self.id)
    await store.resume()
    await store.cancel()
    let counts = await harness.counts()
    XCTAssertEqual(counts, [1, 1])
    XCTAssertEqual(store.status?.original?.sourceEventID, String(repeating: "b", count: 64))
    XCTAssertEqual(store.status?.retraction?.revisionParentID, Self.id)
    XCTAssertEqual(store.status?.replacement.id, Self.id)
    XCTAssertEqual(store.status?.retraction?.form, nil)
  }

  func testLateSelectionCannotRestoreStoppedOrForeignScopeDetails() async {
    let harness = RevisionDetailHarness(value: Self.fixture())
    let store = TeraRevisionDetailStore(operations: harness.operations)
    store.configure(author: Self.author)
    await harness.pauseNext()
    let load = Task { await store.load(Self.id) }
    while await !(harness.waiting) {
      await Task.yield()
    }
    store.configure(author: String(repeating: "c", count: 64))
    await harness.release()
    await load.value
    XCTAssertNil(store.status)
    XCTAssertFalse(store.isWorking)
    await store.load(Self.id)
    XCTAssertNil(store.status)
    XCTAssertNotNil(store.message)
    let counts = await harness.counts()
    XCTAssertEqual(counts, [0, 0])
  }

  func testNewSelectionReplacesSuspendedReadAndCannotBeOverwritten() async {
    let harness = RevisionDetailHarness(value: Self.fixture())
    let store = TeraRevisionDetailStore(operations: harness.operations)
    store.configure(author: Self.author)
    await harness.pauseNext()
    let old = Task { await store.load(Self.id) }
    while await !(harness.waiting) {
      await Task.yield()
    }
    await store.load(Self.id)
    XCTAssertNotNil(store.status)
    XCTAssertFalse(store.isWorking)
    store.stop()
    await harness.release()
    await old.value
    XCTAssertNil(store.status)
    XCTAssertFalse(store.isWorking)
    await store.load(Self.id)
    XCTAssertNotNil(store.status)
    XCTAssertFalse(store.isWorking)
  }

  private static let author = String(repeating: "a", count: 64)
  private static let id = String(repeating: "1", count: 32)

  private static func fixture() -> TeraRevisionStatus {
    let replacement = TeraDraftStatus(id: id, revision: 1, authorPublicKey: author, kind: .add,
                                      commandType: .createUpdate, form: nil, state: .partiallyDelivered,
                                      cardID: String(repeating: "2", count: 64), operationID: id,
                                      createdAtUnixMilliseconds: 1000, updatedAtUnixMilliseconds: 1001,
                                      media: [], settlement: nil, isRevision: true)
    let child = TeraDraftStatus(id: String(repeating: "3", count: 32), revision: 2, authorPublicKey: author,
                                kind: .retraction, commandType: .createUpdate, form: nil, state: .partiallyDelivered,
                                cardID: String(repeating: "4", count: 64), operationID: String(repeating: "5", count: 32),
                                createdAtUnixMilliseconds: 1000, updatedAtUnixMilliseconds: 1002,
                                media: [], settlement: nil, isRevision: false, revisionParentID: id)
    let targets = TeraPublicationTargets(requiresDelivery: false, policy: .all, targets: [
      TeraPublicationTarget(id: String(repeating: "6", count: 64), endpoint: "wss://relay.example",
                            attempted: true, accepted: true, delivered: false, rejected: false,
                            uncertain: false, readBackObservedAtUnixMilliseconds: nil),
    ], readBackAvailable: true, readBackComplete: true)
    let progress = TeraRevisionBranchStatus(stopped: false, canResume: true, canCancel: true, targets: targets)
    return TeraRevisionStatus(operationID: id, replacement: replacement, retraction: child,
                              policy: .replaceThenRetract, phase: .partialEffect,
                              original: TeraRevisionTarget(cardID: child.cardID, sourceEventID: String(repeating: "b", count: 64),
                                                           sourceAddress: nil, authorPublicKey: author),
                              replacementProgress: progress, retractionProgress: progress, canResume: true, canCancel: true)
  }
}

private actor RevisionDetailHarness {
  private var value: TeraRevisionStatus
  private var resumed = 0
  private var cancelled = 0
  private var pause = false
  private var continuation: CheckedContinuation<Void, Never>?
  var waiting: Bool {
    continuation != nil
  }

  init(value: TeraRevisionStatus) {
    self.value = value
  }

  func set(_ value: TeraRevisionStatus) {
    self.value = value
  }

  func pauseNext() {
    pause = true
  }

  func release() {
    continuation?.resume(); continuation = nil
  }

  func counts() -> [Int] {
    [resumed, cancelled]
  }

  nonisolated var operations: TeraRevisionDetailOperations {
    TeraRevisionDetailOperations(load: { _ in await self.load() },
                                 resume: { _ in await self.resume() }, cancel: { _ in await self.cancel() })
  }

  private func load() async -> TeraRevisionStatus {
    let captured = value
    if pause {
      pause = false
      await withCheckedContinuation { continuation = $0 }
    }
    return captured
  }

  private func resume() -> TeraRevisionStatus {
    resumed += 1; return value
  }

  private func cancel() -> TeraRevisionStatus {
    cancelled += 1; return value
  }
}
