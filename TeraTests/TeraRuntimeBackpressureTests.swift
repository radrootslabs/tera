@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraRuntimeBackpressureTests: XCTestCase {
    func testIndependentSubscriptionsUseBoundedNewestBuffers() async throws {
        let harness = RuntimeHarness()
        let client = TeraRuntimeClient(factory: harness.start)
        _ = try await client.start(configuration: TeraRuntimeClientTests().makeConfiguration(generation: "02"))
        let first = try await client.changes(bufferCapacity: 2)
        let second = try await client.changes(bufferCapacity: 4)

        for generation in 1 ... 10 {
            await harness.emitRevision(UInt64(generation))
        }

        _ = try await client.stop()
        var firstValues: [TeraRuntimeChange] = []
        var secondValues: [TeraRuntimeChange] = []
        for await value in first {
          firstValues.append(value)
        }
        for await value in second {
          secondValues.append(value)
        }
        XCTAssertEqual(firstValues.count, 2)
        XCTAssertEqual(secondValues.count, 4)
        for values in [firstValues, secondValues] {
            XCTAssertEqual(values.last?.delivery, .resnapshotRequired)
            XCTAssertEqual(values.last?.scope.context, nil)
            XCTAssertEqual(values.last(where: { $0.delivery == .change })?.revision.rawValue, 10)
        }
        let cancelCount = await harness.cancelCount()
        XCTAssertEqual(cancelCount, 2)
    }

  func testGeneratedCallbackRetainsFinalGapAtEverySupportedBufferBoundary() async {
    for capacity in [1, 2, 16, 64] {
      for overflow in [false, true] {
        let pair = AsyncStream.makeStream(of: TeraRuntimeChange.self, bufferingPolicy: .bufferingNewest(capacity))
        let observer = TeraGeneratedRuntimeObserver(continuation: pair.continuation)
        for revision in 1 ... (capacity + (overflow ? 1 : 0)) {
          observer.onChange(change: generated(revision: UInt64(revision)))
        }
        observer.finish()
        var values: [TeraRuntimeChange] = []
        for await value in pair.stream {
          values.append(value)
        }
        XCTAssertEqual(values.count, capacity)
        if overflow {
          XCTAssertEqual(values.last, generated(revision: 1).appValue.requiringResnapshot())
        } else {
          XCTAssertTrue(values.allSatisfy { $0.delivery == .change })
          XCTAssertEqual(values.map(\.revision.rawValue), (1 ... capacity).map { UInt64($0) })
        }
      }
    }
  }

  func testRepeatedGapDoesNotAdvanceDomainWatermarksOrAcceptAnotherEpoch() async throws {
    let harness = RuntimeHarness()
    let client = TeraRuntimeClient(factory: harness.start)
    _ = try await client.start(configuration: TeraRuntimeClientTests().makeConfiguration(generation: "66"))
    let stream = try await client.changes()
    await harness.emit(generated(revision: 5).appValue)
    let gap = generated(revision: 99).appValue.requiringResnapshot()
    await harness.emit(gap)
    await harness.emit(gap)
    await harness.emit(generated(revision: 6).appValue)
    var otherEpoch = generated(revision: 100)
    otherEpoch.epoch = String(repeating: "2", count: 32)
    await harness.emit(otherEpoch.appValue.requiringResnapshot())
    await harness.emit(generated(revision: 7).appValue)
    _ = try await client.stop()
    var values: [TeraRuntimeChange] = []
    for await value in stream {
      values.append(value)
    }
    XCTAssertEqual(values.map(\.delivery), [.change, .resnapshotRequired, .resnapshotRequired, .change, .change])
    XCTAssertEqual(values.filter { $0.delivery == .change }.map(\.revision.rawValue), [5, 6, 7])
  }

  func testOldScopeGapCannotCrossRuntimeReplacement() async throws {
    let harness = RuntimeHarness()
    let client = TeraRuntimeClient(factory: harness.start)
    _ = try await client.start(configuration: TeraRuntimeClientTests().makeConfiguration(generation: "66"))
    let first = try await client.changes()
    await harness.emit(generated(revision: 5).appValue)
    _ = try await client.start(configuration: TeraRuntimeClientTests().makeConfiguration(generation: "77"))
    let second = try await client.changes()
    await harness.emit(generated(revision: 99).appValue.requiringResnapshot())
    let current = generated(revision: 1, account: "77").appValue
    await harness.emit(current)
    _ = try await client.stop()
    var firstValues: [TeraRuntimeChange] = []
    var secondValues: [TeraRuntimeChange] = []
    for await value in first {
      firstValues.append(value)
    }
    for await value in second {
      secondValues.append(value)
    }
    XCTAssertEqual(firstValues, [generated(revision: 5).appValue])
    XCTAssertEqual(secondValues, [current])
  }

  func testGeneratedDeliveryVariantsRoundTripWithoutChangingScopeOrEpoch() throws {
    for delivery in [FfiRuntimeChangeDelivery.change, .resnapshotRequired] {
      var original = generated(revision: .max)
      original.delivery = delivery
      let decoded = try FfiConverterTypeFfiRuntimeChangeRecord_lift(FfiConverterTypeFfiRuntimeChangeRecord_lower(original))
      XCTAssertEqual(decoded, original)
      XCTAssertEqual(decoded.appValue.delivery, delivery == .change ? .change : .resnapshotRequired)
      let gap = decoded.appValue.requiringResnapshot()
      XCTAssertEqual(gap.scope.publicKey, original.scope.publicKey)
      XCTAssertEqual(gap.scope.sourceGeneration, original.scope.sourceGeneration)
      XCTAssertEqual(gap.epoch, original.epoch)
      XCTAssertNil(gap.scope.context)
      XCTAssertNil(gap.entityID)
      XCTAssertEqual(gap.kind, .initial)
    }
  }

  private func generated(revision: UInt64, account: String = "66") -> FfiRuntimeChangeRecord {
    FfiRuntimeChangeRecord(
      schemaVersion: 3,
      scope: FfiRuntimeChangeScope(
        publicKey: String(repeating: account, count: 32), sourceGeneration: String(repeating: account, count: 32),
        context: FfiLocalNetworkRecord(schemaVersion: 1, id: "local", label: "Local network", relayUrls: ["wss://relay.example"], locality: nil, followedAuthors: [], generation: 1)
      ),
      epoch: String(repeating: "1", count: 32), revision: .current(value: revision), delivery: .change, kind: .today, entityId: "card"
    )
  }
}
