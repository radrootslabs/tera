import Foundation
import SwiftUI
@testable import TeraApp
import XCTest

final class TeraExactDecimalPresentationTests: XCTestCase {
  func testAllAdmittedDigitsSurviveLocaleGroupingWithoutRounding() {
    let raw = "12345678901234567890.12345678"
    for (locale, expected) in [
      ("en_US", "12,345,678,901,234,567,890.12345678"),
      ("de_DE", "12.345.678.901.234.567.890,12345678"),
      ("fr_FR", "12\u{202F}345\u{202F}678\u{202F}901\u{202F}234\u{202F}567\u{202F}890,12345678"),
      ("ar_EG", "١٢٬٣٤٥٬٦٧٨٬٩٠١٬٢٣٤٬٥٦٧٬٨٩٠٫١٢٣٤٥٦٧٨"),
      ("en_IN", "1,23,45,67,89,01,23,45,67,890.12345678"),
    ] {
      XCTAssertEqual(TeraExactDecimalPresentation(locale: Locale(identifier: locale)).string(raw), expected, locale)
    }
    XCTAssertEqual(raw, "12345678901234567890.12345678")
  }

  func testTinyFractionsAndLargestIntegersRetainEveryDigit() {
    let format = TeraExactDecimalPresentation(locale: Locale(identifier: "de_DE"))
    XCTAssertEqual(format.string("0.000000000000000000000000001"), "0,000000000000000000000000001")
    XCTAssertEqual(format.string("9999999999999999999999999999"), "9.999.999.999.999.999.999.999.999.999")
    XCTAssertEqual(format.string("0"), "0")
    for raw in ["", "1.", ".1", "1e3", "1,5", "-1", "١", "1.2.3", String(repeating: "9", count: 29)] {
      XCTAssertNil(format.string(raw), raw)
    }
  }

  func testVisibleAndAccessibleFoodValuesUseTheSameExactLocalePresentation() {
    let card = foodCard()
    let original = card
    let locale = Locale(identifier: "de_DE")
    let format = TeraFoodPresentation(locale: locale)
    XCTAssertEqual(format.price(card), "12.345,67890123456789 ZZZ/bag")
    XCTAssertEqual(format.quantity(card), "0,000000000000000000000000001 bag available")
    for mode: TeraTodayCardPresentation in [.feed, .detail] {
      let summary = mode.accessibility(card, locale: locale, timeZone: .gmt)
      XCTAssertTrue(summary.contains("12.345,67890123456789 ZZZ/bag"))
      XCTAssertTrue(summary.contains("0,000000000000000000000000001 bag available"))
    }
    XCTAssertEqual(card, original)
    XCTAssertEqual(card.priceAmount, "12345.67890123456789")
    XCTAssertEqual(card.priceCurrency, "ZZZ")
    XCTAssertEqual(card.priceUnit, "bag")
  }

  func testAuthoredInstantChecksTheNativeDomainAndLocalYearBeforeConversion() throws {
    let format = TeraCalendarPresentation(locale: Locale(identifier: "en_US"), timeZone: .gmt)
    XCTAssertTrue(format.authoredSummary(0).hasPrefix("Posted: Jan 1, 1970"))
    XCTAssertEqual(format.authoredDate(0), Date(timeIntervalSince1970: 0))
    for seconds: UInt64 in [253_402_300_800, (1 << 53) + 1, .max] {
      XCTAssertEqual(format.authoredSummary(seconds), "Date unavailable")
    }
    let east = try XCTUnwrap(TimeZone(identifier: "Pacific/Kiritimati"))
    XCTAssertEqual(TeraCalendarPresentation(timeZone: east).authoredSummary(253_402_300_799), "Date unavailable")
    XCTAssertNil(TeraCalendarPresentation(timeZone: east).authoredDate(253_402_300_799))
  }

  @MainActor
  func testProductionFoodCardRendersLocalizedValuesAtAccessibleSize() async throws {
    let client = try await TeraScopeFixtures.client(TeraScopeBackend())
    let store = TeraMediaStore(runtimeClient: client)
    let view = TeraTodayCardView(card: foodCard(), context: nil, mediaStore: store, presentation: .detail)
      .environment(\.locale, Locale(identifier: "de_DE"))
      .environment(\.dynamicTypeSize, .accessibility5).padding(16).frame(width: 390)
    let renderer = ImageRenderer(content: view)
    let image = try XCTUnwrap(renderer.uiImage)
    XCTAssertGreaterThan(image.size.height, 100)
    let attachment = XCTAttachment(image: image)
    attachment.name = "c122-exact-food-de-DE-accessibility5"
    attachment.lifetime = .keepAlways
    add(attachment)
    _ = try await client.stop()
  }

  private func foodCard() -> TeraTodayCard {
    TeraTodayCard(
      id: "food", type: .foodAvailability, sourceEventID: "event", sourceAddress: nil,
      authorPublicKey: String(repeating: "a", count: 64), contractID: "test.food",
      title: nil, content: "Seasonal food", authoredAtUnixSeconds: 0, effectiveAtUnixSeconds: 0,
      calendarTiming: nil, location: nil, priceAmount: "12345.67890123456789", priceCurrency: "ZZZ",
      priceUnit: "bag", quantity: "0.000000000000000000000000001", foodSummary: nil,
      foodPublishedAtUnixSeconds: nil, foodStatus: nil, contextRank: 1, inclusionReason: "local",
      media: [], lifecycle: .active, rankDigest: nil, authorProfile: nil, thread: [],
      localOperationID: nil, localOperationState: nil
    )
  }
}
