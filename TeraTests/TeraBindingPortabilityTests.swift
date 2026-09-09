@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraBindingPortabilityTests: XCTestCase {
  func testActualCallbacksPreserveUnsignedContextBoundariesAndScope() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let pair = AsyncStream.makeStream(of: TeraRuntimeChange.self, bufferingPolicy: .bufferingNewest(8))
    let observer = TeraGeneratedRuntimeObserver(continuation: pair.continuation)
    let handle = try runtime.subscribeChanges(observer: observer)
    defer { handle.unsubscribe(); observer.finish() }
    let initial = try await next(pair.stream)
    XCTAssertEqual(initial.kind, .initial)
    XCTAssertEqual(initial.epoch.count, 32)
    for (index, generation) in [UInt64(0), UInt64(1), UInt64(Int64.max) + 1, UInt64.max].enumerated() {
      let context = context(generation: generation)
      _ = try await runtime.phase1RefreshToday(context: context, nowUnixS: 1_800_000_000, update: .incremental)
      let changed = try await next(pair.stream)
      XCTAssertEqual(changed.scope.publicKey, initial.scope.publicKey)
      XCTAssertEqual(changed.scope.sourceGeneration, initial.scope.sourceGeneration)
      XCTAssertEqual(changed.scope.context?.id, context.id)
      XCTAssertEqual(changed.scope.context?.generation, generation)
      XCTAssertEqual(changed.epoch, initial.epoch)
      XCTAssertEqual(changed.revision.rawValue, UInt64(index + 1))
    }
    _ = try await runtime.shutdown()
  }

  func testInvalidContextErrorsCrossTheGeneratedBoundaryWithoutAnInvalidation() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let pair = AsyncStream.makeStream(of: TeraRuntimeChange.self, bufferingPolicy: .bufferingNewest(8))
    let observer = TeraGeneratedRuntimeObserver(continuation: pair.continuation)
    let handle = try runtime.subscribeChanges(observer: observer)
    defer { handle.unsubscribe(); observer.finish() }
    _ = try await next(pair.stream)
    var unsupported = context(generation: 1)
    unsupported.schemaVersion = .max
    var invalid = context(generation: 1)
    invalid.id = ""
    for (context, code) in [(invalid, "invalid_local_network"), (unsupported, "unsupported_schema_version")] {
      do {
        _ = try await runtime.phase1RefreshToday(context: context, nowUnixS: 1_800_000_000, update: .incremental)
        XCTFail("Invalid context must not be admitted")
      } catch let TeraAppError.Failure(report) {
        XCTAssertEqual(report.code, code)
      }
    }
    try runtime.configurePublicRelays(writableRelays: ["wss://write.example"])
    let changed = try await next(pair.stream)
    XCTAssertEqual(changed.kind, .relay)
    _ = try await runtime.shutdown()
  }

  private func context(generation: UInt64) -> FfiLocalNetworkRecord {
    FfiLocalNetworkRecord(schemaVersion: 1, id: "nearby", label: "Nearby", relayUrls: ["wss://relay.example"], locality: nil, followedAuthors: [], generation: generation)
  }

  private func next(_ stream: AsyncStream<TeraRuntimeChange>) async throws -> TeraRuntimeChange {
    let value = await withTaskGroup(of: TeraRuntimeChange?.self) { group in
      group.addTask { await stream.first { _ in true } }
      group.addTask { try? await Task.sleep(for: .seconds(5)); return nil }
      defer { group.cancelAll() }
      return await group.next() ?? nil
    }
    return try XCTUnwrap(value, "Native callback deadline")
  }
}
