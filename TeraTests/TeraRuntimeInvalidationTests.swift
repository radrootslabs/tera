@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraRuntimeInvalidationTests: XCTestCase {
  func testClientRejectsWrongAuthorityAndOldRevisionsWithoutRejectingOtherDomains() async throws {
    let harness = RuntimeHarness()
    let client = TeraRuntimeClient(factory: harness.start)
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "66")
    _ = try await client.start(configuration: configuration)
    let stream = try await client.changes(bufferCapacity: 8)
    await harness.emit(change(revision: 2))
    for schema in [UInt16(1), UInt16(3)] {
      await harness.emit(change(revision: 9, schema: schema))
    }
    await harness.emit(change(revision: 9, account: "bb"))
    await harness.emit(change(revision: 9, storage: "cc"))
    await harness.emit(change(revision: 9, epoch: "2"))
    await harness.emit(change(revision: 9, epoch: "g"))
    await harness.emit(change(revision: 2))
    await harness.emit(change(revision: 1))
    await harness.emit(change(revision: 1, kind: .media))
    await harness.emit(change(revision: 3))
    await harness.emit(change(revision: nil))
    await harness.emit(change(revision: .max))
    await harness.emit(change(revision: nil))
    _ = try await client.stop()
    var received: [TeraRuntimeChange] = []
    for await value in stream {
      received.append(value)
    }
    XCTAssertEqual(received.map(\.kind), [.today, .media, .today, .today, .today])
    XCTAssertEqual(received.map(\.revision.rawValue), [2, 1, 3, nil, nil])
  }

  func testNewSubscriptionAdmitsItsOwnEpochAndDoesNotReuseOldWatermarks() async throws {
    let harness = RuntimeHarness()
    let client = TeraRuntimeClient(factory: harness.start)
    _ = try await client.start(configuration: TeraRuntimeClientTests().makeConfiguration(generation: "66"))
    let first = try await client.changes()
    await harness.emit(change(revision: .max))
    let second = try await client.changes()
    await harness.emit(change(revision: 1, epoch: "2"))
    _ = try await client.stop()
    var firstValues: [TeraRuntimeChange] = []
    var secondValues: [TeraRuntimeChange] = []
    for await value in first {
      firstValues.append(value)
    }
    for await value in second {
      secondValues.append(value)
    }
    XCTAssertEqual(firstValues, [change(revision: .max)])
    XCTAssertEqual(secondValues, [change(revision: 1, epoch: "2")])
  }

  func testGeneratedWireRoundTripPreservesBothRevisionStatesAndFullScope() throws {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "66")
    for revision in [FfiInvalidationRevision.current(value: .max), .exhausted] {
      let record = generated(revision: revision)
      let decoded = try FfiConverterTypeFfiRuntimeChangeRecord_lift(
        FfiConverterTypeFfiRuntimeChangeRecord_lower(record)
      )
      XCTAssertEqual(decoded, record)
      let value = decoded.appValue
      XCTAssertTrue(value.matches(configuration))
      XCTAssertEqual(value.scope.context?.label, "Marché 農")
      XCTAssertEqual(value.scope.context?.generation, UInt64.max)
      XCTAssertEqual(value.entityID, "draft")
      XCTAssertEqual(value.revision.rawValue, revision == .exhausted ? nil : UInt64.max)
      XCTAssertTrue(value.matches(context: value.scope.context))
      XCTAssertFalse(value.matches(context: nil))
      let other = TeraLocalNetwork(
        schemaVersion: 1, id: "local", label: "Marché 農", relayURLs: ["wss://other.example"],
        locality: "Town", followedAuthors: [String(repeating: "c", count: 64)], generation: .max
      )
      XCTAssertFalse(value.matches(context: other))
      XCTAssertTrue(change(revision: 1).matches(context: other))
    }
  }

  private func change(
    revision: UInt64?, kind: TeraRuntimeChangeKind = .today, schema: UInt16 = 2,
    account: String = "66", storage: String = "66", epoch: String = "1"
  ) -> TeraRuntimeChange {
    TeraRuntimeChange(
      schemaVersion: schema,
      scope: TeraRuntimeChangeScope(
        publicKey: String(repeating: account, count: 32),
        sourceGeneration: String(repeating: storage, count: 32), context: nil
      ),
      epoch: String(repeating: epoch, count: 32), revision: TeraProjectionRevision(rawValue: revision),
      kind: kind, entityID: nil
    )
  }

  private func generated(revision: FfiInvalidationRevision) -> FfiRuntimeChangeRecord {
    FfiRuntimeChangeRecord(
      schemaVersion: 2,
      scope: FfiRuntimeChangeScope(
        publicKey: String(repeating: "66", count: 32),
        sourceGeneration: String(repeating: "66", count: 32),
        context: FfiLocalNetworkRecord(
          schemaVersion: 1, id: "local", label: "Marché 農", relayUrls: ["wss://relay.example"],
          locality: "Town", followedAuthors: [String(repeating: "c", count: 64)], generation: .max
        )
      ),
      epoch: String(repeating: "1", count: 32), revision: revision, kind: .today, entityId: "draft"
    )
  }
}
