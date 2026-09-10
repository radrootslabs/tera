import Foundation
@testable import TeraApp
import XCTest

final class TeraCalendarEditingTests: XCTestCase {
  func testCivilComponentsRetainIncompleteInputAndCanonicalizeWithoutDateParsing() throws {
    let partial = TeraCivilDateInput(raw: "2024--")
    XCTAssertNil(partial.value)
    let month = partial.replacing(1, with: "2")
    XCTAssertEqual(month, "2024-2-")
    let leap = TeraCivilDateInput(raw: month).replacing(2, with: "29")
    XCTAssertEqual(leap, "2024-2-29")
    XCTAssertEqual(TeraCivilDateInput(raw: leap).canonicalOrRaw, "2024-02-29")
    XCTAssertEqual(TeraCivilDateInput(raw: leap).value, try TeraCivilDate(year: 2024, month: 2, day: 29))
    XCTAssertNil(TeraCivilDateInput(raw: "2023-02-29").value)
    XCTAssertNil(TeraCivilDateInput(raw: "2026-09-05-extra").value)
    let arabic = TeraCivilDateInput(raw: "٢٠٢٦-٠٩-").replacing(2, with: "٠٥")
    XCTAssertEqual(arabic, "٢٠٢٦-٠٩-٠٥")
    XCTAssertEqual(TeraCivilDateInput(raw: arabic).canonicalOrRaw, "2026-09-05")
    XCTAssertEqual(TeraCivilDateInput(raw: "2026-09-05").replacing(0, with: "2"), "2-09-05")
    XCTAssertNil(TeraCivilDateInput(raw: "--").replacing(0, with: ""))
  }

  func testNewFormUsesLocalCivilTodayAndSeparateCheckedInstantDefaults() throws {
    let zone = try XCTUnwrap(TimeZone(identifier: "America/Vancouver"))
    let form = TeraAddPresentation.newForm(type: .createEvent, identifier: { String(repeating: "a", count: 32) },
                                           clock: .fixed(unixSeconds: 1_788_568_200), timeZone: zone)
    XCTAssertEqual(form.eventTimezone, "America/Vancouver")
    XCTAssertEqual(form.eventStartDate, "2026-09-04")
    XCTAssertEqual(form.eventEndDate, "2026-09-05")
    XCTAssertEqual(form.eventStartUnixSeconds, 1_788_571_800)
    XCTAssertEqual(form.eventEndUnixSeconds, 1_788_575_400)
  }

  func testExclusiveNextDayUsesGregorianComponentsAtLeapAndDomainBoundaries() throws {
    XCTAssertEqual(try TeraCivilDate(year: 2024, month: 2, day: 28).nextDay?.canonical, "2024-02-29")
    XCTAssertEqual(try TeraCivilDate(year: 2024, month: 2, day: 29).nextDay?.canonical, "2024-03-01")
    XCTAssertEqual(try TeraCivilDate(year: 1900, month: 2, day: 28).nextDay?.canonical, "1900-03-01")
    XCTAssertEqual(try TeraCivilDate(year: 2025, month: 12, day: 31).nextDay?.canonical, "2026-01-01")
    XCTAssertNil(try TeraCivilDate(year: 9999, month: 12, day: 31).nextDay)
  }

  func testHistoricalSpringGapNeverBecomesANormalizedInstant() throws {
    let zone = try XCTUnwrap(TimeZone(identifier: "America/Vancouver"))
    let gap = try TeraWallTime(date: TeraCivilDate(year: 2024, month: 3, day: 10), hour: 2, minute: 30)
    XCTAssertEqual(gap.instants(in: zone), [])
    XCTAssertEqual(TeraWallTime(date: gap.date, hour: 3, minute: 30).instants(in: zone).count, 1)
    XCTAssertEqual(TeraWallTime(date: gap.date, hour: 25, minute: 0).instants(in: zone), [])
  }

  func testHistoricalOverlapOffersBothExactInstantsWithoutDiscardingSeconds() throws {
    let zone = try XCTUnwrap(TimeZone(identifier: "America/Vancouver"))
    let wall = try TeraWallTime(date: TeraCivilDate(year: 2024, month: 11, day: 3), hour: 1, minute: 30, second: 17)
    let expected: [UInt64] = [1_730_622_617, 1_730_626_217]
    XCTAssertEqual(wall.instants(in: zone), expected)
    for instant in expected {
      XCTAssertEqual(TeraWallTime.from(instant, timeZone: zone)?.instants(in: zone), expected)
      let date = try XCTUnwrap(TeraCalendarEditing.pickerInstant(instant))
      XCTAssertEqual(TeraCalendarEditing.pickerSeconds(date), instant)
    }
  }

  func testSkippedLocalDayRemainsAValidCivilDateAndHasNoTimedInstant() throws {
    let zone = try XCTUnwrap(TimeZone(identifier: "Pacific/Apia"))
    let date = try TeraCivilDate(year: 2011, month: 12, day: 30)
    XCTAssertEqual(TeraCivilDateInput(raw: date.canonical).value, date)
    XCTAssertEqual(TeraWallTime(date: date, hour: 12, minute: 0).instants(in: zone), [])
    XCTAssertEqual(date.nextDay?.canonical, "2011-12-31")
  }

  func testUnsupportedUnsignedValuesAndDatesCannotSilentlySeedOrRoundPickers() {
    for value: UInt64 in [0, 253_402_300_800, (1 << 53) + 1, .max] {
      XCTAssertNil(TeraCalendarEditing.pickerInstant(value))
    }
    for value in [Double.nan, .infinity, -1, 253_402_300_800] {
      XCTAssertNil(TeraCalendarEditing.pickerSeconds(Date(timeIntervalSince1970: value)))
    }
    var form = TeraAddForm.empty(.createEvent)
    TeraCalendarEditing.initialize(&form, now: .max, timeZone: .gmt)
    XCTAssertNil(form.eventStartDate)
    XCTAssertNil(form.eventEndDate)
    XCTAssertNil(form.eventStartUnixSeconds)
    XCTAssertNil(form.eventEndUnixSeconds)
  }

  @MainActor
  func testModeAndZoneChangesPreserveEveryOtherUnsavedField() async throws {
    let client = try await TeraScopeFixtures.client(TeraScopeBackend())
    let store = TeraAddStore(runtimeClient: client, initialType: .createEvent, clock: .fixed(unixSeconds: 1_788_568_200))
    store.updateForm(\.title, "Unsaved harvest")
    store.updateForm(\.content, "Unsaved details")
    store.updateForm(\.eventStartDate, "2026-09-")
    store.updateForm(\.eventEndDate, "2026-09-07")
    store.updateForm(\.eventTimezone, "America/Vancouver")
    let original = store.form
    var expected = original
    expected.eventTiming = .allDay
    store.updateForm(\.eventTiming, .allDay)
    XCTAssertEqual(store.form, expected)
    store.updateForm(\.eventTiming, .timed)
    XCTAssertEqual(store.form, original)
    expected = original
    expected.eventTimezone = "Pacific/Kiritimati"
    store.updateForm(\.eventTimezone, "Pacific/Kiritimati")
    XCTAssertEqual(store.form, expected)
    _ = try await client.stop()
  }
}
