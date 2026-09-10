import TeraKitBindings
import XCTest

final class TeraTodayReconciliationFFITests: XCTestCase {
  func testGeneratedReconciliationCarriesCurrentGenerationAndRejectsInvalidSelection() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let context = FfiLocalNetworkRecord(
      schemaVersion: 1, id: "nearby", label: "Nearby", relayUrls: ["wss://relay.example"],
      locality: nil, followedAuthors: [], generation: 1
    )
    let page = try await runtime.phase1TodayPage(context: context, limit: 20, asOfUnixS: 1_900_000_000, cursor: nil, viewerTimeZone: "UTC")
    let current = try await runtime.phase1TodayReconcile(context: context, asOfUnixS: page.asOfUnixS, cardIds: [], expectedGeneration: page.projectionGeneration, viewerTimeZone: "UTC")
    XCTAssertEqual(current.projectionGeneration, page.projectionGeneration)
    XCTAssertEqual(current.items, page.items)
    XCTAssertNil(current.nextCursor)
    do {
      _ = try await runtime.phase1TodayReconcile(context: context, asOfUnixS: page.asOfUnixS, cardIds: [String(repeating: "a", count: 65)], expectedGeneration: nil, viewerTimeZone: "UTC")
      XCTFail("Invalid card identity must fail at the generated runtime boundary")
    } catch let TeraAppError.Failure(report) {
      XCTAssertFalse(report.code.isEmpty)
    }
    _ = try await runtime.shutdown()
  }
}
