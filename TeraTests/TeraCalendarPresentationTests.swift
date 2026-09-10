import Foundation
import SwiftUI
@testable import TeraApp
import XCTest

final class TeraCalendarPresentationTests: XCTestCase {
  private let locale = Locale(identifier: "en_US")

  func testSeptemberFifthNeverMovesAcrossDeviceTimeZones() throws {
    let timing = try civil(2026, 9, 5)
    for name in ["America/Vancouver", "UTC", "Pacific/Kiritimati", "Pacific/Pago_Pago", "Asia/Kathmandu"] {
      let zone = try XCTUnwrap(TimeZone(identifier: name))
      XCTAssertEqual(TeraCalendarPresentation(locale: locale, timeZone: zone).summary(timing), "All day, Sep 5, 2026")
    }
  }

  func testCivilLocaleOrderMonthNamesAndNumeralsUseTheSameGregorianDate() throws {
    let value = try TeraCivilDate(year: 2026, month: 9, day: 5)
    for (identifier, expected) in [("en_US", "Sep 5, 2026"), ("en_GB", "5 Sep 2026"),
                                   ("fr_FR", "5 sept. 2026"), ("ja_JP", "2026年9月5日")]
    {
      XCTAssertEqual(TeraCivilDateFormat(locale: Locale(identifier: identifier)).string(value), expected, identifier)
    }
    let arabic = TeraCivilDateFormat(locale: Locale(identifier: "ar_EG")).string(value)
    XCTAssertTrue(arabic.contains("٥"))
    XCTAssertTrue(arabic.contains("٢٠٢٦"))
    XCTAssertEqual(value.canonical, "2026-09-05")
  }

  func testExclusiveCivilEndsDisplayOnlyIncludedLeapAndYearBoundaryDays() throws {
    let format = TeraCalendarPresentation(locale: locale)
    XCTAssertEqual(try format.summary(civil(2024, 2, 29, end: date(2024, 3, 1))), "All day, Feb 29, 2024")
    XCTAssertEqual(try format.summary(civil(2024, 2, 28, end: date(2024, 3, 1))), "All day, Feb 28, 2024 – Feb 29, 2024")
    XCTAssertEqual(try format.summary(civil(2026, 9, 5, end: date(2026, 9, 7))), "All day, Sep 5, 2026 – Sep 6, 2026")
    XCTAssertEqual(try format.summary(civil(2025, 12, 31, end: date(2026, 1, 2))), "All day, Dec 31, 2025 – Jan 1, 2026")
    XCTAssertEqual(try format.summary(civil(2026, 9, 5, end: date(2026, 9, 5))), "Date unavailable")
  }

  func testCivilPredecessorHandlesGregorianCenturyAndSupportedExtremes() throws {
    for (year, last) in [(UInt16(1900), UInt8(28)), (2000, 29), (2100, 28), (2400, 29)] {
      XCTAssertEqual(try TeraCivilDate(year: year, month: 3, day: 1).previousDay,
                     try TeraCivilDate(year: year, month: 2, day: last))
    }
    XCTAssertNil(try TeraCivilDate(year: 1, month: 1, day: 1).previousDay)
    XCTAssertEqual(try TeraCivilDate(year: 9999, month: 12, day: 31).previousDay,
                   try TeraCivilDate(year: 9999, month: 12, day: 30))
    XCTAssertTrue(try TeraCalendarPresentation(locale: locale).summary(civil(9999, 12, 31)).contains("9999"))
  }

  func testTimedDisplayRetainsExactInstantsAndDistinctSourceZones() throws {
    let timing = TeraCalendarTiming.timeBased(startUnixSeconds: 0, endExclusiveUnixSeconds: 3600,
                                              startTimezoneID: "America/Vancouver", endTimezoneID: "Europe/Paris")
    let zone = try XCTUnwrap(TimeZone(identifier: "UTC"))
    let text = TeraCalendarPresentation(locale: locale, timeZone: zone).summary(timing)
    XCTAssertTrue(text.contains("Starts: Jan 1, 1970"))
    XCTAssertTrue(text.contains("Ends: Jan 1, 1970"))
    XCTAssertTrue(text.contains("Display time zone: \(zone.identifier)"))
    XCTAssertTrue(text.contains("Event start: Dec 31, 1969"))
    XCTAssertTrue(text.contains("(America/Vancouver)"))
    XCTAssertTrue(text.contains("Event end: Jan 1, 1970"))
    XCTAssertTrue(text.contains("(Europe/Paris)"))
    XCTAssertFalse(text.contains("All day"))
    XCTAssertEqual(timing, .timeBased(startUnixSeconds: 0, endExclusiveUnixSeconds: 3600,
                                      startTimezoneID: "America/Vancouver", endTimezoneID: "Europe/Paris"))
  }

  func testFullUnsignedWireValuesNeverBecomeRoundedOrInventedDisplayDates() {
    for seconds: UInt64 in [253_402_300_800, (1 << 53) + 1, .max] {
      let timing = TeraCalendarTiming.timeBased(startUnixSeconds: seconds, endExclusiveUnixSeconds: nil,
                                                startTimezoneID: nil, endTimezoneID: nil)
      let text = TeraCalendarPresentation(locale: locale).summary(timing)
      XCTAssertTrue(text.contains("Date unavailable"))
      XCTAssertFalse(text.contains("1970"))
      XCTAssertFalse(text.contains("Event start"))
    }
  }

  func testRepeatedWallTimesRetainDaylightSavingOffsetsAndLocalYearBounds() throws {
    let zone = try XCTUnwrap(TimeZone(identifier: "America/Vancouver"))
    let format = TeraCalendarPresentation(locale: locale, timeZone: zone)
    // Use a historical overlap: current tzdata keeps Vancouver on PDT in late 2026.
    let timing = TeraCalendarTiming.timeBased(startUnixSeconds: 1_730_622_600, endExclusiveUnixSeconds: 1_730_626_200,
                                              startTimezoneID: nil, endTimezoneID: nil)
    let text = format.summary(timing)
    XCTAssertTrue(text.contains("PDT"), text)
    XCTAssertTrue(text.contains("PST"), text)
    let east = try XCTUnwrap(TimeZone(identifier: "Pacific/Kiritimati"))
    let upper = TeraCalendarTiming.timeBased(startUnixSeconds: 253_402_300_799, endExclusiveUnixSeconds: nil,
                                             startTimezoneID: nil, endTimezoneID: nil)
    XCTAssertTrue(TeraCalendarPresentation(locale: locale, timeZone: east).summary(upper).contains("Date unavailable"))
  }

  func testFeedAndDetailAccessibilityIncludeTheIdenticalVisibleTiming() throws {
    let zone = try XCTUnwrap(TimeZone(identifier: "America/Vancouver"))
    for timing in try [civil(2026, 9, 5, end: date(2026, 9, 7)),
                       .timeBased(startUnixSeconds: 0, endExclusiveUnixSeconds: 3600,
                                  startTimezoneID: "Europe/Paris", endTimezoneID: nil)]
    {
      let card = TeraScopeFixtures.card("Calendar example", timing: timing)
      let visible = TeraCalendarPresentation(locale: locale, timeZone: zone).summary(timing)
      for presentation: TeraTodayCardPresentation in [.feed, .detail] {
        XCTAssertTrue(presentation.accessibility(card, locale: locale, timeZone: zone).contains(visible))
      }
      XCTAssertTrue(card.accessibilitySummary(locale: locale, timeZone: zone).contains(visible))
    }
  }

  @MainActor
  func testProductionCalendarCardScreenshotsRetainRangeAndAccessibleText() async throws {
    let client = try await TeraScopeFixtures.client(TeraScopeBackend())
    let store = TeraMediaStore(runtimeClient: client)
    let zone = try XCTUnwrap(TimeZone(identifier: "America/Vancouver"))
    let card = try TeraScopeFixtures.card("Community harvest", timing: civil(2026, 9, 5, end: date(2026, 9, 7)))
    for (name, presentation, size) in [("feed", TeraTodayCardPresentation.feed, DynamicTypeSize.large),
                                       ("detail-accessible", .detail, .accessibility3)]
    {
      XCTAssertTrue(presentation.accessibility(card, locale: locale, timeZone: zone)
        .contains("All day, Sep 5, 2026 – Sep 6, 2026"))
      let content = TeraTodayCardView(card: card, context: nil, mediaStore: store, presentation: presentation)
        .environment(\.locale, locale).environment(\.timeZone, zone)
        .environment(\.dynamicTypeSize, size).environment(\.colorScheme, .light)
        .padding(16).frame(width: 390).background(.white)
      let renderer = ImageRenderer(content: content)
      renderer.scale = 2
      let image = try XCTUnwrap(renderer.uiImage)
      XCTAssertEqual(image.size.width, 390)
      XCTAssertGreaterThan(image.size.height, 100)
      let attachment = XCTAttachment(image: image)
      attachment.name = "c050-calendar-\(name)"
      attachment.lifetime = .keepAlways
      add(attachment)
    }
    _ = try await client.stop()
  }

  private func civil(_ year: UInt16, _ month: UInt8, _ day: UInt8,
                     end: TeraCivilDate? = nil) throws -> TeraCalendarTiming
  {
    try .dateBased(start: date(year, month, day), endExclusive: end)
  }

  private func date(_ year: UInt16, _ month: UInt8, _ day: UInt8) throws -> TeraCivilDate {
    try TeraCivilDate(year: year, month: month, day: day)
  }
}
