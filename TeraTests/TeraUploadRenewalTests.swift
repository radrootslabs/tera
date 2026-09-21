import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraUploadRenewalTests: XCTestCase {
  func testLineageUsesExactSavedRevisionsAndRequiresEvidenceForLegacyAttempts() throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let operation = String(repeating: "a", count: 32)
    let job = fixture.job(revision: 2, operation: operation)
    let prior = TeraUploadAttemptIdentity(operationID: operation, revision: nil, expirationUnixSeconds: 100)
    XCTAssertThrowsError(try TeraUploadRenewal.identities([prior], parent: fixture.draftID, inventory: []))
    let snapshot = try RadrootsBackgroundTransferSnapshot(request: fixture.request(job: job), state: .expired)
    let observed = try TeraUploadRenewal.identities([prior], parent: fixture.draftID, inventory: [snapshot])
    XCTAssertEqual(observed.map(\.revision), [2])
    XCTAssertEqual(observed.map(\.identifier.rawValue), [job.transferIdentifier])
    let incorrect = TeraUploadAttemptIdentity(operationID: operation, revision: 3, expirationUnixSeconds: 100)
    XCTAssertThrowsError(try TeraUploadRenewal.identities([incorrect], parent: fixture.draftID, inventory: [snapshot]))
    XCTAssertThrowsError(try TeraUploadRenewal.identities(Array(repeating: prior, count: 6), parent: fixture.draftID, inventory: [snapshot]))
    let next = TeraUploadAttemptIdentity(operationID: String(repeating: "b", count: 32), revision: 4, expirationUnixSeconds: 200)
    let lineage = try TeraUploadRenewal.identities([prior, next], parent: fixture.draftID, inventory: [snapshot])
    XCTAssertEqual(lineage.map(\.revision), [2, 4])
    XCTAssertNotEqual(lineage[0].identifier, lineage[1].identifier)
  }

  func testActualNativeGuardRefusesActiveUnknownAndReceiptBeforeRenewalBody() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let request = try fixture.request(job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)))
    let prior = TeraUploadRenewal.Prior(identifier: request.identifier, attempt: String(repeating: "a", count: 32), revision: 2)
    for scenario in 0 ..< 3 {
      let store = fixture.nativeStore
      let snapshot = try RadrootsBackgroundTransferSnapshot(request: request, state: scenario == 2 ? .awaitingVerification : .expired)
      try await store.saveSnapshot(snapshot)
      let adapters = RadrootsAppleBackgroundTransferAdapters(enqueue: { _, _ in XCTFail("Unexpected enqueue") },
                                                             cancel: { _ in XCTFail("Unexpected cancel") }, activeTransferIdentifiers: {
          if scenario == 1 {
            throw RadrootsBackgroundTransferError.transferFailure
          }
          return scenario == 0 ? [request.identifier] : []
        }, handleBackgroundEvents: { _, done in done() })
      let transfer = RadrootsAppleBackgroundTransfer(store: store, adapters: adapters)
      do {
        try await TeraUploadRenewal.holding([prior], transfer: transfer) { _ in XCTFail("Unsafe renewal admitted") }
        XCTFail("Unreconciled execution must be refused")
      } catch {}
      let after = try await store.loadSnapshots()
      XCTAssertEqual(after, [snapshot])
    }
  }

  func testAllPriorIdentifiersStayReservedWhileActualFileStoreRetainsLateBody() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let store = fixture.nativeStore
    let adapters = RadrootsAppleBackgroundTransferAdapters(enqueue: { _, _ in XCTFail("Unexpected enqueue") },
                                                           cancel: { _ in XCTFail("Unexpected cancel") }, activeTransferIdentifiers: { [] }, handleBackgroundEvents: { _, done in done() })
    let transfer = RadrootsAppleBackgroundTransfer(store: store, adapters: adapters)
    let other = RadrootsAppleBackgroundTransfer(store: fixture.nativeStore, adapters: adapters)
    let requests = try ["a", "b"].enumerated().map { offset, char in
      try fixture.request(job: fixture.job(revision: UInt64(offset + 2), operation: String(repeating: char, count: 32)))
    }
    let originals = try requests.map { try RadrootsBackgroundTransferSnapshot(request: $0, state: .expired, executionID: UUID()) }
    for original in originals {
      try await store.saveSnapshot(original)
    }
    let lineage = try originals.map { original -> TeraUploadRenewal.Prior in
      let identity = try XCTUnwrap(TeraBackgroundUploadRequest.transferIdentity(original.identifier))
      return .init(identifier: original.identifier, attempt: identity.attempt, revision: identity.revision)
    }
    let late = try RadrootsBackgroundTransferSnapshot(request: requests[0], state: .awaitingVerification,
                                                      response: RadrootsBackgroundTransferResponse(statusCode: 200, mediaType: "application/json", body: Data("{\"late\":true}".utf8)),
                                                      executionID: originals[0].executionID)
    try await TeraUploadRenewal.holding(lineage, transfer: transfer) { snapshots in
      XCTAssertEqual(snapshots, originals)
      for request in requests {
        do { _ = try await other.retry(request); XCTFail("Prior identifier was not reserved") } catch {}
      }
      let saved = try await fixture.nativeStore.compareExchangeSnapshot(expected: originals[0], desired: late)
      XCTAssertTrue(saved)
    }
    let retained = try await fixture.nativeStore.loadSnapshots().first { $0.identifier == requests[0].identifier }
    XCTAssertEqual(retained, late)
    do {
      try await TeraUploadRenewal.holding(lineage, transfer: other) { _ in XCTFail("Late response cannot be replaced") }
      XCTFail("Late receipt must be reconciled first")
    } catch {}
  }
}
