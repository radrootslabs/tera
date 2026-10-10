import TeraKitBindings
import XCTest

final class TeraSearchFFICompatibilityTests: XCTestCase {
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
