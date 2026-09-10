import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraViewerCalendarTests: XCTestCase {
  func testGeneratedPageRetainsExactContextAndRejectsUnsupportedReaders() throws {
    let context = FfiViewerCalendarContext(
      schemaVersion: 1, asOfUnixS: 1_788_569_400, timeZone: "America/Vancouver",
      civilDate: FfiCivilDate(year: 2026, month: 9, day: 4)
    )
    let page = FfiTodayPageRecord(
      calendar: context, projectionGeneration: 9, schemaVersion: 2,
      asOfUnixS: context.asOfUnixS, items: [], nextCursor: "opaque"
    )
    let restored = try FfiConverterTypeFfiTodayPageRecord_lift(FfiConverterTypeFfiTodayPageRecord_lower(page))
    XCTAssertEqual(restored, page)
    let native = try restored.appValue()
    XCTAssertEqual(native.calendar.civilDate.canonical, "2026-09-04")
    XCTAssertEqual(native.calendar.timeZoneID, "America/Vancouver")
    for index in 0 ..< 7 {
      var invalid = page
      switch index {
      case 0: invalid.schemaVersion = 1
      case 1: invalid.calendar.schemaVersion = 2
      case 2: invalid.calendar.timeZone = "Invalid/Zone"
      case 3: invalid.calendar.civilDate.day = 32
      case 4: invalid.asOfUnixS += 1
      case 5: invalid.calendar.asOfUnixS = .max
      default: invalid.calendar.asOfUnixS = 0
      }
      XCTAssertThrowsError(try invalid.appValue())
    }
  }

  func testInstalledRuntimeDerivesLocalDateAndRejectsOldCursor() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let context = FfiLocalNetworkRecord(
      schemaVersion: 1, id: "nearby", label: "Nearby", relayUrls: ["wss://relay.example"],
      locality: nil, followedAuthors: [], generation: 1
    )
    let page = try await runtime.phase1TodayPage(
      context: context, limit: 20, asOfUnixS: 1_788_569_400, cursor: nil,
      viewerTimeZone: "America/Vancouver"
    )
    XCTAssertEqual(try page.appValue().calendar.civilDate.canonical, "2026-09-04")
    let current = try await runtime.phase1TodayReconcile(
      context: context, asOfUnixS: page.asOfUnixS, cardIds: [],
      expectedGeneration: page.projectionGeneration, viewerTimeZone: page.calendar.timeZone
    )
    XCTAssertEqual(current.calendar, page.calendar)
    do {
      _ = try await runtime.phase1TodayPage(
        context: context, limit: 20, asOfUnixS: nil, cursor: "rrtc2:00", viewerTimeZone: nil
      )
      XCTFail("An incompatible cursor requires an explicit fresh read")
    } catch let TeraAppError.Failure(report) {
      XCTAssertEqual(report.code, "today_cursor_invalid")
      XCTAssertEqual(report.recoveryActions, ["restart_pagination"])
    }
    _ = try await runtime.shutdown()
  }
}
