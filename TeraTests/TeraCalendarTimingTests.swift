import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraCalendarTimingTests: XCTestCase {
  func testGeneratedNativeBuffersPreserveCivilDatesAndExclusiveEnds() throws {
    let values: [FfiCalendarTiming] = [
      .dateBased(start: FfiCivilDate(year: 1, month: 1, day: 1), endExclusive: nil),
      .dateBased(start: FfiCivilDate(year: 2024, month: 2, day: 29), endExclusive: FfiCivilDate(year: 2024, month: 3, day: 1)),
      .dateBased(start: FfiCivilDate(year: 2026, month: 9, day: 5), endExclusive: FfiCivilDate(year: 2026, month: 9, day: 7)),
      .dateBased(start: FfiCivilDate(year: 9999, month: 12, day: 31), endExclusive: nil),
    ]
    for value in values {
      let restored = try FfiConverterTypeFfiCalendarTiming_lift(FfiConverterTypeFfiCalendarTiming_lower(value))
      XCTAssertEqual(restored, value)
      guard case let .dateBased(start, end) = restored,
            case let .dateBased(nativeStart, nativeEnd) = try restored.appValue()
      else { return XCTFail("The generated boundary must retain civil timing.") }
      XCTAssertEqual(nativeStart.year, start.year)
      XCTAssertEqual(nativeStart.month, start.month)
      XCTAssertEqual(nativeStart.day, start.day)
      XCTAssertEqual(nativeEnd?.year, end?.year)
      XCTAssertEqual(nativeEnd?.month, end?.month)
      XCTAssertEqual(nativeEnd?.day, end?.day)
    }
  }

  func testGeneratedNativeBuffersKeepUnsignedInstantsAndBothZonesWithoutRounding() throws {
    for start: UInt64 in [0, (1 << 53) + 1, .max - 1, .max] {
      let end = start == .max ? nil : start + 1
      for zones in [false, true] {
        let value = FfiCalendarTiming.timeBased(
          startUnixS: start, endExclusiveUnixS: end,
          startTzid: zones ? "America/Vancouver" : nil, endTzid: zones ? "Europe/Paris" : nil
        )
        let restored = try FfiConverterTypeFfiCalendarTiming_lift(FfiConverterTypeFfiCalendarTiming_lower(value))
        XCTAssertEqual(restored, value)
        guard case let .timeBased(actualStart, actualEnd, startZone, endZone) = try restored.appValue() else {
          return XCTFail("The generated boundary must retain timed values.")
        }
        XCTAssertEqual(actualStart, start)
        XCTAssertEqual(actualEnd, end)
        XCTAssertEqual(startZone, zones ? "America/Vancouver" : nil)
        XCTAssertEqual(endZone, zones ? "Europe/Paris" : nil)
      }
    }
    XCTAssertNotNil(TeraCalendarTiming.presentationInstant(0))
    XCTAssertNotNil(TeraCalendarTiming.presentationInstant(253_402_300_799))
    XCTAssertNil(TeraCalendarTiming.presentationInstant(253_402_300_800))
    XCTAssertNil(TeraCalendarTiming.presentationInstant(.max))
  }

  func testUnsupportedGeneratedDiscriminantsAndTruncatedPayloadsThrow() {
    for bytes: [UInt8] in [[0, 0, 0, 99], [0, 0, 0, 1], []] {
      var input = (data: Data(bytes), offset: 0)
      XCTAssertThrowsError(try FfiConverterTypeFfiCalendarTiming.read(from: &input))
    }
  }

  func testMalformedNativeTimingAndUnsupportedCardSchemasFailThroughTypedRecovery() throws {
    let date = FfiCivilDate(year: 2026, month: 9, day: 5)
    let invalid: [FfiCalendarTiming] = [
      .dateBased(start: FfiCivilDate(year: 0, month: 1, day: 1), endExclusive: nil),
      .dateBased(start: FfiCivilDate(year: 2023, month: 2, day: 29), endExclusive: nil),
      .dateBased(start: FfiCivilDate(year: 2024, month: 13, day: 1), endExclusive: nil),
      .dateBased(start: date, endExclusive: date),
      .timeBased(startUnixS: 2, endExclusiveUnixS: 1, startTzid: nil, endTzid: nil),
      .timeBased(startUnixS: 1, endExclusiveUnixS: nil, startTzid: "Mars/Olympus", endTzid: nil),
    ]
    for timing in invalid {
      XCTAssertThrowsError(try timing.appValue()) { error in
        XCTAssertEqual(error as? TeraRuntimeFailure, TeraCalendarTiming.unsupported)
      }
    }
    let timing = FfiCalendarTiming.dateBased(start: date, endExclusive: nil)
    for version: UInt16 in [0, 1, .max] {
      XCTAssertThrowsError(try card(schema: version, timing: timing).appValue())
    }
    XCTAssertThrowsError(try card(schema: 2, timing: nil).appValue())
    let valid = card(schema: 2, timing: timing)
    let restored = try FfiConverterTypeFfiTodayCardRecord_lift(FfiConverterTypeFfiTodayCardRecord_lower(valid))
    XCTAssertEqual(restored, valid)
    XCTAssertEqual(try restored.appValue().calendarTiming, try timing.appValue())
  }

  private func card(schema: UInt16, timing: FfiCalendarTiming?) -> FfiTodayCardRecord {
    FfiTodayCardRecord(
      schemaVersion: schema, cardId: "synthetic-card", cardType: .event, sourceEventId: "synthetic-event",
      sourceAddress: nil, authorPublicKey: "synthetic-author", contractId: "synthetic-calendar",
      title: "Harvest day", content: "Synthetic calendar", authoredAtUnixS: 1, effectiveAtUnixS: 1,
      calendarTiming: timing, location: nil, priceAmount: nil, priceCurrency: nil, priceUnit: nil,
      quantity: nil, foodSummary: nil, foodPublishedAtUnixS: nil, foodStatus: nil, contextRank: 1,
      inclusionReason: "local", media: [], lifecycle: .active, rankDigest: nil, authorProfile: nil,
      thread: [], localOperationId: nil, localOperationState: nil
    )
  }
}
