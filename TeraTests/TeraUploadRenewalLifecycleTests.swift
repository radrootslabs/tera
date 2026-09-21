import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraUploadRenewalLifecycleTests: XCTestCase {
  func testCallerDeadlineKeepsNativeReservationUntilLateWorkerReturns() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "35")
    let backend = ResourceTestBackend(publicKeyHex: configuration.publicKeyHex)
    let client = TeraRuntimeClient(factory: { _ in await backend.start() },
                                   deadlines: TeraRuntimeDeadlinePolicy(operationNanoseconds: 20_000_000))
    _ = try await client.start(configuration: configuration)
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    let store = fixture.nativeStore
    try await store.saveSnapshot(RadrootsBackgroundTransferSnapshot(request: request, state: .expired))
    let adapters = RadrootsAppleBackgroundTransferAdapters(enqueue: { _, _ in XCTFail("Unexpected enqueue") },
                                                           cancel: { _ in XCTFail("Unexpected cancel") }, activeTransferIdentifiers: { [] }, handleBackgroundEvents: { _, done in done() })
    let transfer = RadrootsAppleBackgroundTransfer(store: store, adapters: adapters)
    let other = RadrootsAppleBackgroundTransfer(store: fixture.nativeStore, adapters: adapters)
    let pause = ResourceTestPause()
    let task = Task {
      try await client.withUploadRenewal { _ in
        try await transfer.withInactiveExecution(for: request.identifier) { _ in
          await pause.wait()
          throw TeraComposerAcknowledgment.unconfirmed
        }
      }
    }
    await pause.entered.wait()
    do { _ = try await task.value; XCTFail("Deadline must return to caller") } catch {}
    do { _ = try await other.retry(request); XCTFail("Late worker must retain the old identifier") } catch {}
    do {
      _ = try await client.withUploadRenewal { _ in XCTFail("Duplicate worker admitted"); throw TeraComposerAcknowledgment.unconfirmed }
      XCTFail("Submission admission must remain held")
    } catch {}
    await pause.resume.open()
    _ = try await client.stop()
    // Shutdown drains the actual worker before this new owner can acquire it.
    let reopened = try await other.withInactiveExecution(for: request.identifier) { snapshot in snapshot?.identifier }
    XCTAssertEqual(reopened, request.identifier)
    let snapshot = try await store.loadSnapshots().first { $0.identifier == request.identifier }
    XCTAssertEqual(snapshot?.state, .expired)
  }
}
