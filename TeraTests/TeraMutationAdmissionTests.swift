import RadrootsKit
@testable import TeraApp
import XCTest

@MainActor
final class TeraMutationAdmissionTests: XCTestCase {
  func testUploadReservesBeforeDiscoveryAndKeepsOtherDraftsIndependent() async throws {
    let fixture = try BackgroundUploadFixture()
    let other = try BackgroundUploadFixture(draftID: String(repeating: "2", count: 32))
    defer { fixture.remove(); other.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let pause = ResourceTestPause()
    await transfer.pauseDiscovery(pause)
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    let owner = Task { try await coordinator.uploadInBackground(job: job, media: fixture.media) }
    await entered(pause)
    await assertBusy(coordinator, job: job, media: fixture.media)
    await assertBusy(
      coordinator,
      job: fixture.job(revision: 3, operation: String(repeating: "b", count: 32)),
      media: fixture.media
    )
    let independent = try await coordinator.uploadInBackground(
      job: other.job(revision: 2, operation: String(repeating: "c", count: 32)),
      media: other.media
    )
    XCTAssertEqual(independent.draftID, other.draftID)
    let discoveries = await transfer.discoveryCount
    XCTAssertEqual(discoveries, 2)
    await pause.resume.open()
    let receipt = try await owner.value
    XCTAssertEqual(receipt.draftID, fixture.draftID)
    let counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 2)
    XCTAssertEqual(counts.retry, 0)
  }

  func testRetryRetainsAdmissionAcrossTheNativeCallback() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    try await transfer.seed(request: fixture.request(job: job), state: .interrupted)
    let pause = ResourceTestPause()
    await transfer.pauseRetry(pause)
    let owner = Task { try await coordinator.uploadInBackground(job: job, media: fixture.media) }
    await entered(pause)
    await assertBusy(coordinator, job: job, media: fixture.media)
    await pause.resume.open()
    _ = try await owner.value
    let counts = await transfer.counts
    XCTAssertEqual(counts.retry, 1)
    XCTAssertEqual(counts.enqueue, 0)
  }

  func testCancellationBeforeAdmissionDoesNotEnterNativeWorkOrRetainTheScope() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    let queued = ResourceTestPause()
    let waiter = Task {
      await queued.wait()
      return try await coordinator.uploadInBackground(job: job, media: fixture.media)
    }
    await entered(queued)
    waiter.cancel()
    await queued.resume.open()
    do {
      _ = try await waiter.value
      XCTFail("A cancelled queued caller must not enter native work")
    } catch is CancellationError {}
    let discoveries = await transfer.discoveryCount
    XCTAssertEqual(discoveries, 0)
    _ = try await coordinator.uploadInBackground(job: job, media: fixture.media)
    let counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 1)
  }

  func testCancelledOwnerKeepsAdmissionUntilItsNativeWaitReturns() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    let pause = ResourceTestPause()
    await transfer.pauseDiscovery(pause)
    let owner = Task { try await coordinator.uploadInBackground(job: job, media: fixture.media) }
    await entered(pause)
    owner.cancel()
    await assertBusy(coordinator, job: job, media: fixture.media)
    await pause.resume.open()
    do {
      _ = try await owner.value
      XCTFail("Expected cancellation before enqueue")
    } catch is CancellationError {}
    var counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 0)
    _ = try await coordinator.uploadInBackground(job: job, media: fixture.media)
    counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 1)
  }

  func testReentrantNativeCallbackCannotReadmitTheSameDraft() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    let callback = expectation(description: "Reentrant callback returned")
    await transfer.onDiscovery {
      do {
        _ = try await coordinator.uploadInBackground(job: job, media: fixture.media)
        XCTFail("A callback must not acquire its caller's scope")
      } catch let failure as TeraRuntimeFailure {
        XCTAssertEqual(failure.code, "operation_in_progress")
      } catch {
        XCTFail("Unexpected reentrant failure")
      }
      callback.fulfill()
    }
    _ = try await coordinator.uploadInBackground(job: job, media: fixture.media)
    await fulfillment(of: [callback], timeout: 2)
    let counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 1)
    let discoveries = await transfer.discoveryCount
    XCTAssertEqual(discoveries, 1)
  }

  private func assertBusy(
    _ coordinator: TeraAddMediaCoordinator,
    job: TeraNativeUploadJob,
    media: TeraPreparedMedia
  ) async {
    do {
      _ = try await coordinator.uploadInBackground(job: job, media: media)
      XCTFail("Overlapping calls must preserve the admitted operation")
    } catch let failure as TeraRuntimeFailure {
      XCTAssertEqual(failure.code, "operation_in_progress")
      XCTAssertEqual(failure.category, "operation")
      XCTAssertTrue(failure.retryable)
      XCTAssertEqual(failure.recoveryActions, ["retry_operation_with_same_idempotency_key"])
    } catch {
      XCTFail("Unexpected admission failure")
    }
  }

  private func entered(_ pause: ResourceTestPause) async {
    let arrived = expectation(description: "Entered the explicit native pause")
    let observer = Task { await pause.entered.wait(); arrived.fulfill() }
    await fulfillment(of: [arrived], timeout: 2)
    await pause.entered.open()
    await observer.value
  }
}

extension TeraMutationAdmissionTests {
  @MainActor
  func testProfileAdmissionSurvivesStopUntilTheAdmittedCallReturns() async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "41")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let client = TeraRuntimeClient(factory: { _ in await backend.start() })
    _ = try await client.start(configuration: configuration)
    let store = TeraSettingsStore(runtimeClient: client)
    store.profileName = "Farm profile"
    let pause = ResourceTestPause()
    await backend.pauseProfile(pause)
    let owner = Task { await store.saveProfile() }
    await entered(pause)
    store.profileName = "A conflicting profile update"
    await store.saveProfile()
    store.stop()
    await store.saveProfile()
    var count = await backend.profileMutations
    XCTAssertEqual(count, 1)
    await pause.resume.open()
    await owner.value
    XCTAssertNil(store.profileStatus)
    await store.saveProfile()
    count = await backend.profileMutations
    XCTAssertEqual(count, 2)
    XCTAssertNotNil(store.profileStatus)
    _ = try await client.stop()
  }

  @MainActor
  func testProfileAdvanceRejectsConflictsAndCancelledQueuedCallers() async throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "41")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let client = TeraRuntimeClient(factory: { _ in await backend.start() })
    _ = try await client.start(configuration: configuration)
    let store = TeraSettingsStore(runtimeClient: client)
    await store.saveProfile()
    let pause = ResourceTestPause()
    await backend.pauseProfile(pause)
    let owner = Task { await store.advanceProfile() }
    await entered(pause)
    await store.cancelProfile()
    await store.saveProfile()
    await pause.resume.open()
    await owner.value
    let queued = ResourceTestPause()
    let waiter = Task { await queued.wait(); await store.saveProfile() }
    await entered(queued)
    waiter.cancel()
    await queued.resume.open()
    await waiter.value
    let count = await backend.profileMutations
    XCTAssertEqual(count, 2)
    XCTAssertFalse(store.isWorking)
    _ = try await client.stop()
  }
}
