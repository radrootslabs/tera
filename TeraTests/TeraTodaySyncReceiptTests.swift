@testable import TeraApp
import TeraKitBindings
import XCTest

@MainActor
final class TeraTodaySyncReceiptTests: XCTestCase {
  func testPartialAndOfflineReceiptsRemainIndependentOfCachedContentAndProjectionFreshness() async throws {
    for state: TeraTodayRelaySyncState in [.partial, .offline] {
      let backend = try TeraScopeBackend()
      let receipt = TeraTodaySyncFixtures.receipt(state: state, targets: [target()])
      await backend.setSyncReceipt(receipt)
      let client = try await TeraScopeFixtures.client(backend)
      let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
      store.configure(snapshot: TeraScopeFixtures.snapshot())
      await store.reload(refreshProjection: false)
      let cached = store.cards
      await store.reload()
      XCTAssertEqual(store.cards, cached)
      XCTAssertEqual(store.presentation.content, .available)
      XCTAssertEqual(store.presentation.refresh, .completed)
      XCTAssertEqual(store.presentation.relayReceipt, receipt)
      XCTAssertEqual(store.presentation.freshness, .refreshed(contentGeneration: 1))
      XCTAssertTrue(store.presentation.accessibilityStatus.contains("Relay 1: Some requested posts were unavailable."))
      XCTAssertFalse(store.presentation.accessibilityStatus.contains("Refresh failed."))
      _ = try await client.stop()
    }
  }

  func testLateReceiptFromOldContextCannotReplaceNewContextEvidence() async throws {
    let backend = try TeraScopeBackend()
    await backend.setSyncReceipt(TeraTodaySyncFixtures.receipt(state: .offline))
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    let pause = await backend.pause(.refresh)
    let old = Task { await store.reload() }
    await pause.entered.wait()
    store.configure(snapshot: TeraScopeFixtures.snapshot(relay: "second"))
    XCTAssertNil(store.presentation.relayReceipt)
    let current = TeraTodaySyncFixtures.receipt(state: .partial, targets: [target()])
    await backend.setSyncReceipt(current)
    await store.reload()
    await pause.resume.open()
    await old.value
    XCTAssertEqual(store.presentation.relayReceipt, current)
    XCTAssertEqual(store.cards.map(\.id), ["wss://second.example"])
    _ = try await client.stop()
  }

  func testCancelledRefreshCannotInstallALatePartialReceipt() async throws {
    let backend = try TeraScopeBackend()
    await backend.setSyncReceipt(TeraTodaySyncFixtures.receipt(state: .partial))
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.reload(refreshProjection: false)
    let cached = store.cards
    let pause = await backend.pause(.refresh)
    let task = Task { await store.reload() }
    await pause.entered.wait()
    task.cancel()
    await task.value
    await pause.resume.open()
    XCTAssertNil(store.presentation.relayReceipt)
    XCTAssertEqual(store.cards, cached)
    XCTAssertEqual(store.presentation.refresh, .idle)
    _ = try await client.stop()
  }

  func testGeneratedRecordRetainsCumulativeEvidenceAndProjectionCounts() {
    let record = FfiTodaySyncRecord(
      schemaVersion: 1, relayState: .partial, termination: .pageLimit,
      targets: [FfiTodayTargetSyncRecord(
        targetFingerprint: "opaque", finalState: .complete,
        summary: FfiTodayTargetPageSummary(
          pagesObserved: 8, incompletePages: 1, missingOutcomePages: 0, lastIncomplete: .partial
        )
      )], pagesFetched: 8, eventsObserved: 500, eventsAdmitted: 490, eventsRejected: 10,
      projection: FfiTodayRefreshRecord(
        schemaVersion: 1, update: .rebuild, sourceEvents: 501, visibleCards: 400,
        profiles: 20, threadEntries: 70, contentGeneration: 7, changed: true
      )
    )
    let receipt = record.appValue
    XCTAssertEqual(receipt.relayState, .partial)
    XCTAssertEqual(receipt.termination, .pageLimit)
    XCTAssertEqual(receipt.targets.first?.finalState, .complete)
    XCTAssertEqual(receipt.targets.first?.summary?.lastIncomplete, .partial)
    XCTAssertEqual(receipt.targets.first?.summary?.pagesObserved, 8)
    XCTAssertEqual(receipt.targets.first?.summary?.incompletePages, 1)
    XCTAssertEqual(receipt.targets.first?.targetFingerprint, "opaque")
    XCTAssertEqual(receipt.pagesFetched, 8)
    XCTAssertEqual(receipt.eventsObserved, 500)
    XCTAssertEqual(receipt.eventsAdmitted, 490)
    XCTAssertEqual(receipt.eventsRejected, 10)
    XCTAssertEqual(receipt.projection.update, .rebuild)
    XCTAssertEqual(receipt.projection.sourceEvents, 501)
    XCTAssertEqual(receipt.projection.visibleCards, 400)
    XCTAssertEqual(receipt.projection.profiles, 20)
    XCTAssertEqual(receipt.projection.threadEntries, 70)
    XCTAssertEqual(receipt.projection.contentGeneration, 7)
    XCTAssertTrue(receipt.projection.changed)
  }

  func testGeneratedEnumMappingsAndUnknownEvidenceDoNotInventCompletion() {
    let states: [FfiTodayTargetSyncState] = [.complete, .partial, .unavailable, .failedRetryable, .failedTerminal, .cancelled]
    XCTAssertEqual(states.map(\.appValue), [.complete, .partial, .unavailable, .failedRetryable, .failedTerminal, .cancelled])
    let ends: [FfiTodaySyncTermination] = [.complete, .pageLimit, .deadline, .cancelled, .sourceFailed]
    XCTAssertEqual(ends.map(\.appValue), [.complete, .pageLimit, .deadline, .cancelled, .sourceFailed])
    let relayStates: [FfiTodayRelaySyncState] = [.complete, .partial, .offline]
    XCTAssertEqual(relayStates.map(\.appValue), [.complete, .partial, .offline])
    let unknown = FfiTodayTargetSyncRecord(targetFingerprint: "opaque", finalState: nil, summary: nil).appValue
    XCTAssertNil(unknown.summary)
    XCTAssertNil(unknown.finalState)
    XCTAssertEqual(unknown.statusMessage, "Response unconfirmed.")
    let omitted = TeraTodayTargetSyncReceipt(
      targetFingerprint: "opaque", finalState: .complete,
      summary: TeraTodayTargetPageSummary(pagesObserved: 2, incompletePages: 0, missingOutcomePages: 1, lastIncomplete: nil)
    )
    XCTAssertEqual(omitted.statusMessage, "Some responses unconfirmed.")
  }

  private func target() -> TeraTodayTargetSyncReceipt {
    TeraTodayTargetSyncReceipt(
      targetFingerprint: "opaque", finalState: .complete,
      summary: TeraTodayTargetPageSummary(
        pagesObserved: 2, incompletePages: 1, missingOutcomePages: 0, lastIncomplete: .partial
      )
    )
  }
}
