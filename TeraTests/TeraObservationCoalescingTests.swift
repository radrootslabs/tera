@testable import TeraApp
import XCTest

@MainActor
final class TeraObservationCoalescingTests: XCTestCase {
  func testRecoveryResnapshotsWithoutAnInitialHintOrAnotherEvent() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let observation = TeraStoreObservation()
    let retry = ResourceTestPause()
    var batches: [TeraObservationBatch] = []
    observation.start(
      client: client, buffer: (capacity: 8, delay: { _ in await retry.wait() }), state: { _ in },
      accepts: { _ in true }, refresh: { batches.append($0) }
    )
    await TeraScopeFixtures.eventually { batches.count == 1 }
    await client.suspend()
    await retry.entered.wait()
    XCTAssertEqual(batches.count, 1)
    await retry.resume.open()
    await TeraScopeFixtures.eventually { batches.count == 2 }
    XCTAssertTrue(batches.allSatisfy(\.resnapshot))
    let subscriptions = await backend.counts[.subscribe]
    XCTAssertEqual(subscriptions, 2)
    observation.stop()
    _ = try await client.stop()
  }

  func testStormCoalescesFixedDomainsAndDisplaysOneFinalRevision() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let observation = TeraStoreObservation()
    let firstRead = ResourceTestPause()
    let kinds: [TeraRuntimeChangeKind] = [.today, .drafts, .media, .settings, .identity, .profile, .relay, .lifecycle]
    var batches: [TeraObservationBatch] = []
    let durable = ObservationTestState()
    var visibleRevisions: [Int] = []
    var seen: UInt64 = 0
    observation.start(
      client: client, buffer: (capacity: 8, delay: { _ in throw CancellationError() }), state: { _ in },
      accepts: { seen = $0.revision.rawValue ?? seen; return true },
      refresh: { batch in
        batches.append(batch)
        let captured = durable.revision
        if batches.count == 1 {
          await firstRead.wait()
        }
        visibleRevisions.append(captured)
      }
    )
    await firstRead.entered.wait()
    for index in 1 ... 256 {
      durable.revision = index
      await backend.emit(kinds[(index - 1) % kinds.count])
      await TeraScopeFixtures.eventually { seen == UInt64(index) }
    }
    XCTAssertEqual(batches.count, 1)
    await firstRead.resume.open()
    await TeraScopeFixtures.eventually { visibleRevisions.count == 2 }
    XCTAssertEqual(visibleRevisions, [0, 256])
    XCTAssertEqual(try XCTUnwrap(batches.last).domains, Set(kinds))
    XCTAssertFalse(try XCTUnwrap(batches.last).resnapshot)
    observation.stop()
    _ = try await client.stop()
  }

  func testGapOrExhaustedRevisionForcesFinalSnapshotAfterSilence() async throws {
    for exhausted in [false, true] {
      let backend = try TeraScopeBackend()
      let client = try await TeraScopeFixtures.client(backend)
      let observation = TeraStoreObservation()
      let first = ResourceTestPause()
      var seen = false
      var batches: [TeraObservationBatch] = []
      observation.start(
        client: client, buffer: (capacity: 1, delay: { _ in throw CancellationError() }), state: { _ in },
        accepts: { _ in seen = true; return true }, refresh: {
          batches.append($0)
          if batches.count == 1 {
            await first.wait()
          }
        }
      )
      await first.entered.wait()
      await backend.emit(.initial, delivery: exhausted ? .change : .resnapshotRequired, exhausted: exhausted)
      await TeraScopeFixtures.eventually { seen }
      XCTAssertEqual(batches.count, 1)
      await first.resume.open()
      await TeraScopeFixtures.eventually { batches.count == 2 }
      XCTAssertTrue(try XCTUnwrap(batches.last).resnapshot)
      XCTAssertTrue(try XCTUnwrap(batches.last).domains.isEmpty)
      observation.stop()
      _ = try await client.stop()
    }
  }

  func testOldRefreshCompletionCannotClearReplacementOrItsPendingWork() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let observation = TeraStoreObservation()
    let oldRead = ResourceTestPause()
    let newRead = ResourceTestPause()
    let oldFinished = ResourceTestGate()
    observation.start(
      client: client, buffer: (capacity: 8, delay: { _ in throw CancellationError() }), state: { _ in },
      accepts: { _ in true }, refresh: { _ in await oldRead.wait(); await oldFinished.open() }
    )
    await oldRead.entered.wait()
    observation.stop()
    var seen = false
    var newBatches: [TeraObservationBatch] = []
    observation.start(
      client: client, buffer: (capacity: 8, delay: { _ in throw CancellationError() }), state: { _ in },
      accepts: { _ in seen = true; return true }, refresh: {
        newBatches.append($0)
        if newBatches.count == 1 {
          await newRead.wait()
        }
      }
    )
    await newRead.entered.wait()
    await oldRead.resume.open()
    await oldFinished.wait()
    await backend.emit(.drafts)
    await TeraScopeFixtures.eventually { seen }
    XCTAssertEqual(newBatches.count, 1)
    await newRead.resume.open()
    await TeraScopeFixtures.eventually { newBatches.count == 2 }
    XCTAssertEqual(try XCTUnwrap(newBatches.last).domains, [.drafts])
    observation.stop()
    _ = try await client.stop()
  }

  func testOldCompletionCannotOrphanReplacementRefreshFromStop() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let observation = TeraStoreObservation()
    let oldRead = ResourceTestPause()
    let newRead = ResourceTestPause()
    let newFinished = ResourceTestGate()
    weak var oldLifetime: ObservationTaskLifetime?
    do {
      let lifetime = ObservationTaskLifetime()
      oldLifetime = lifetime
      observation.start(
        client: client, buffer: (capacity: 8, delay: { _ in throw CancellationError() }), state: { _ in },
        accepts: { _ in true }, refresh: { [lifetime] _ in
          await oldRead.wait()
          withExtendedLifetime(lifetime) {}
        }
      )
    }
    await oldRead.entered.wait()
    observation.stop()
    var cancelled: Bool?
    observation.start(
      client: client, buffer: (capacity: 8, delay: { _ in throw CancellationError() }), state: { _ in },
      accepts: { _ in true }, refresh: { _ in
        await newRead.wait()
        cancelled = Task.isCancelled
        await newFinished.open()
      }
    )
    await newRead.entered.wait()
    await oldRead.resume.open()
    await TeraScopeFixtures.eventually { oldLifetime == nil }
    observation.stop()
    await newRead.resume.open()
    await newFinished.wait()
    XCTAssertEqual(cancelled, true, "Stop must still own and cancel the replacement refresh")
    _ = try await client.stop()
  }
}

@MainActor
private final class ObservationTaskLifetime {}

@MainActor
private final class ObservationTestState {
  var revision = 0
}
