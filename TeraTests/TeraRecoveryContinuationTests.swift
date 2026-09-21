import Foundation
import RadrootsKit
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraRecoveryContinuationTests: XCTestCase {
  func testGeneratedCursorSurvivesRuntimeReopenAndRejectsStaleOrMalformedInput() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let backend = TeraGeneratedRuntimeBackend(runtime: runtime)
    let first = try await backend.nativeRecoverySchedule()
    let saved = try await backend.advanceNativeRecoverySchedule(expected: first, after: String(repeating: "b", count: 64))
    XCTAssertEqual(saved.revision, 1)
    _ = try await runtime.shutdown()
    let reopened = try await fixture.runtime()
    let next = TeraGeneratedRuntimeBackend(runtime: reopened)
    let retained = try await next.nativeRecoverySchedule()
    XCTAssertEqual(retained, saved)
    do {
      _ = try await next.advanceNativeRecoverySchedule(expected: first, after: nil)
      XCTFail("Stale observation cannot rewind traversal")
    } catch {}
    for key in ["", String(repeating: "B", count: 64), String(repeating: "b", count: 65)] {
      do {
        _ = try await next.advanceNativeRecoverySchedule(expected: saved, after: key)
        XCTFail("Malformed cursor must fail")
      } catch {}
    }
    let reset = try await next.advanceNativeRecoverySchedule(expected: saved, after: nil)
    XCTAssertEqual(reset.revision, 2)
    XCTAssertNil(reset.after)
    _ = try await reopened.shutdown()
  }

  func testInterruptedBatchPreservesVisitedPositionAndRetainsUnconfirmedReceipts() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = try await Self.inventory(fixture, count: 130)
    let cursor = NativeRecoveryScheduleTestStorage()
    let lookups = RecoveryContinuationLookups()
    let pause = ResourceTestPause()
    let worker = Task {
      try await TeraNativeRecoveryInventory.run(transfer: transfer, cursor: nil, checkpoint: { key in
        let prior = await cursor.load()
        _ = try await cursor.advance(expected: prior, after: key)
      }) { key in
        if await lookups.record(key) == 2 {
          await pause.wait()
        }
        return nil
      }
    }
    await pause.entered.wait()
    worker.cancel()
    await pause.resume.open()
    do { _ = try await worker.value; XCTFail("Cancelled batch must stop") } catch is CancellationError {}
    let saved = await cursor.load()
    XCTAssertNotNil(saved.after)
    let resumed = try await TeraNativeRecoveryInventory.run(transfer: transfer, cursor: saved.after) { _ in nil }
    XCTAssertEqual(resumed.progress.visited, 64)
    XCTAssertEqual(resumed.progress.remaining, 65)
    let counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 0)
    XCTAssertEqual(counts.enqueue, 0)
    let snapshots = try await transfer.snapshots()
    XCTAssertEqual(snapshots.count, 130)
  }

  @MainActor
  func testFreshCoordinatorContinuesPastQuarantinedPrefix() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = try await Self.inventory(fixture, count: 130)
    let backend = try TeraScopeBackend()
    await backend.setDrafts([])
    let client = try await TeraScopeFixtures.client(backend)
    for remaining in [66, 2, 0] {
      let freshOwner = fixture.coordinator(transfer: transfer)
      let progress = try await freshOwner.recoverNativeUploads(client: client)
      XCTAssertEqual(progress.remaining, remaining)
      XCTAssertTrue(progress.needsAttention)
      XCTAssertLessThanOrEqual(progress.visited, 64)
    }
    let cursor = try await client.nativeRecoverySchedule()
    XCTAssertNil(cursor.after)
    let snapshots = try await transfer.snapshots()
    XCTAssertEqual(snapshots.count, 130, "Traversal never deletes quarantined evidence")
    _ = try await client.stop()
  }

  private static func inventory(_ fixture: BackgroundUploadFixture, count: Int) async throws -> BackgroundTransferHarness {
    let transfer = BackgroundTransferHarness()
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    for index in 1 ... count {
      let identifier = try RadrootsBackgroundTransferIdentifier("radroots.add.\(String(format: "%032x", index)).2.\(String(repeating: "a", count: 32))")
      try await transfer.seed(request: TeraBackgroundUploadRequest.replacingIdentifier(in: request, with: identifier), state: .awaitingVerification)
    }
    return transfer
  }
}

private actor RecoveryContinuationLookups {
  private var count = 0
  func record(_: String) -> Int {
    count += 1; return count
  }
}
