@testable import TeraApp
import XCTest

final class TeraTodayRenderingTests: XCTestCase {
  func testExcerptCharacterAndByteLimitsPreserveFullDetail() {
    let feed = TeraTodayCardPresentation.feed
    let detail = TeraTodayCardPresentation.detail
    let maximum = String(repeating: "a", count: 640)
    XCTAssertEqual(feed.content(maximum), maximum)
    XCTAssertEqual(feed.content(maximum + "b").count, 640)
    XCTAssertTrue(feed.content(maximum + "b").hasSuffix("…"))
    for value in [String(repeating: "👨‍👩‍👧‍👦", count: 1000),
                  "e" + String(repeating: "\u{301}", count: 100_000),
                  String(repeating: "農", count: 2000), String(repeating: "👩🏽‍🌾", count: 1000)]
    {
      let excerpt = feed.content(value)
      XCTAssertLessThanOrEqual(excerpt.utf8.count, 4096)
      XCTAssertLessThanOrEqual(excerpt.count, 640)
      XCTAssertFalse(excerpt.contains("�"))
      XCTAssertEqual(detail.content(value), value)
    }
  }

  func testByteBoundaryNeverIncludesAPartialGrapheme() {
    let cluster = "👩🏽‍🌾"
    let value = "abc" + cluster + "end"
    for bytes in 3 ... value.utf8.count {
      let result = TeraTodayCardPresentation.excerpt(value, characters: 100, bytes: bytes)
      XCTAssertLessThanOrEqual(result.utf8.count, bytes)
      XCTAssertTrue(value.hasPrefix(String(result.dropLast())) || result == value)
      XCTAssertFalse(result.contains("�"))
    }
  }

  func testFeedBoundsLabelsAndThumbnailsWhileDetailRetainsEveryReference() {
    let label = String(repeating: "a", count: 160)
    XCTAssertEqual(TeraTodayCardPresentation.feed.label(label), label)
    XCTAssertEqual(TeraTodayCardPresentation.feed.label(label + "b").count, 160)
    let references = Array(repeating: TeraScopeFixtures.reference(), count: 4)
    XCTAssertEqual(TeraTodayCardPresentation.feed.media(Array(references.prefix(3))).count, 3)
    XCTAssertEqual(TeraTodayCardPresentation.feed.media(references).count, 3)
    XCTAssertEqual(TeraTodayCardPresentation.detail.media(references), references)
  }
}
