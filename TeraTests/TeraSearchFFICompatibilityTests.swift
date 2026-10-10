@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraSearchFFICompatibilityTests: XCTestCase {
  func testInstalledContextEnforcesAggregateAdmission() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let authors = (0 ..< 4096).map { index in
      let suffix = String(index, radix: 16)
      return String(repeating: "0", count: 64 - suffix.count) + suffix
    }
    let context = network(authors: authors)
    XCTAssertEqual(try runtime.phase1LocalNetwork(context: context).followedAuthors, authors)
    for invalid in [
      authors + [String(repeating: "0", count: 60) + "1000"],
      [String(repeating: "a", count: 64), String(repeating: "a", count: 64)],
      [String(repeating: "A", count: 64)], [String(repeating: "x", count: 262_145)],
    ] {
      do {
        _ = try await runtime.phase1Search(context: network(authors: invalid), query: "carrot", limit: 20, asOfUnixS: 1_800_000_000, viewerTimeZone: "UTC")
        XCTFail("Invalid public context must fail before query execution")
      } catch let TeraAppError.Failure(report) {
        XCTAssertEqual(report.code, "invalid_local_network")
        XCTAssertFalse(report.retryable)
        XCTAssertEqual(report.recoveryActions, ["correct_input"])
      }
    }
    _ = try await runtime.shutdown()
    do {
      _ = try await runtime.phase1Search(context: context, query: "carrot", limit: 20, asOfUnixS: 1_800_000_000, viewerTimeZone: "UTC")
      XCTFail("Valid-context closed runtime retains lifecycle precedence")
    } catch let TeraAppError.Failure(report) {
      XCTAssertEqual(report.code, "client_closed")
    }
  }

  func testHostContextDebugRedactsRawFields() {
    let context = TeraLocalNetwork(
      schemaVersion: 1, id: "private-id-sentinel", label: "private-label-sentinel",
      relayURLs: ["wss://private-relay-sentinel.example"], locality: "private-location-sentinel",
      followedAuthors: [String(repeating: "a", count: 64)], generation: 7
    )
    let debug = String(reflecting: context)
    for sentinel in [context.id, context.label, context.relayURLs[0], context.locality ?? "", context.followedAuthors[0]] {
      XCTAssertFalse(debug.contains(sentinel))
    }
    XCTAssertTrue(debug.contains("relayCount: 1"))
    XCTAssertTrue(debug.contains("followedAuthorCount: 1"))
    XCTAssertTrue(debug.contains("generation: 7"))
    XCTAssertEqual(context.label, "private-label-sentinel")
  }

  private func network(authors: [String]) -> FfiLocalNetworkRecord {
    FfiLocalNetworkRecord(
      schemaVersion: 1, id: "nearby", label: "Nearby", relayUrls: ["wss://relay.example"],
      locality: nil, followedAuthors: authors, generation: 1
    )
  }

  func testInstalledSearchEnforcesRawAndNormalizedByteAdmission() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let context = FfiLocalNetworkRecord(
      schemaVersion: 1, id: "nearby", label: "Nearby", relayUrls: ["wss://relay.example"],
      locality: nil, followedAuthors: [], generation: 1
    )
    _ = try await runtime.phase1RefreshToday(context: context, nowUnixS: 1_800_000_000, update: .rebuild)
    for query in [
      String(repeating: "x", count: 257), String(repeating: "é", count: 128) + "a",
      String(repeating: " ", count: 1_048_576) + "carrots", String(repeating: "x", count: 1_048_576),
      "   ", "\ncarrots", "carrots\0", String(repeating: "İ", count: 86),
    ] {
      do {
        _ = try await runtime.phase1Search(context: context, query: query, limit: 20, asOfUnixS: 1_800_000_000, viewerTimeZone: "UTC")
        XCTFail("Invalid query of \(query.utf8.count) raw bytes must fail at the installed boundary")
      } catch let TeraAppError.Failure(report) {
        XCTAssertEqual(report.code, "today_invalid_request")
        XCTAssertFalse(report.retryable)
        XCTAssertEqual(report.recoveryActions, ["correct_input"])
      }
    }
    for query in [
      String(repeating: "x", count: 256), String(repeating: "é", count: 128),
      String(repeating: "İ", count: 85) + "a", "  CARROT  ",
    ] {
      let results = try await runtime.phase1Search(context: context, query: query, limit: 20, asOfUnixS: 1_800_000_000, viewerTimeZone: "UTC")
      XCTAssertTrue(results.isEmpty)
    }
    _ = try await runtime.shutdown()
    do {
      _ = try await runtime.phase1Search(context: context, query: String(repeating: "x", count: 257), limit: 20, asOfUnixS: 1_800_000_000, viewerTimeZone: "UTC")
      XCTFail("Closed runtime admission must retain precedence")
    } catch let TeraAppError.Failure(report) {
      XCTAssertEqual(report.code, "client_closed")
    }
  }
}
