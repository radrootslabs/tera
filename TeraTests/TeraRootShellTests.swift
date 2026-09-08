@testable import TeraApp
import XCTest

final class TeraRootShellTests: XCTestCase {
    func testRootInventoryIsExactlyTodayThenAdd() {
        XCTAssertEqual(TeraRootTab.allCases.map(\.rawValue), ["today", "add"])
    }

    func testRestorationFailsClosedToToday() {
        XCTAssertEqual(TeraRootTab.resolve(nil), .today)
        XCTAssertEqual(TeraRootTab.resolve("capture"), .today)
        XCTAssertEqual(TeraRootTab.resolve("activity"), .today)
        XCTAssertEqual(TeraRootTab.resolve("settings"), .today)
    }

    func testDeepLinksAcceptOnlyCanonicalRootAuthorities() throws {
        for (rawValue, expected) in [
          ("radroots://today", TeraRootTab.today),
          ("RADROOTS://TODAY", TeraRootTab.today),
          ("radroots://add", TeraRootTab.add),
          ("RadRoots://AdD", TeraRootTab.add),
        ] {
            XCTAssertEqual(
              try TeraRootTab.resolve(url: XCTUnwrap(URL(string: rawValue))),
              expected,
              "Expected canonical root deep link: \(rawValue)"
            )
        }
    }

    func testDeepLinksRejectRemovedAndStructurallyAmbiguousRoutes() throws {
        for rawValue in [
          "radroots://capture",
          "radroots://activity",
          "radroots://settings",
          "radroots://search",
          "radroots://me",
          "radroots:today",
          "radroots:/today",
          "radroots:///today",
          "radroots://host/today",
          "radroots://today/",
          "radroots://today/add",
          "radroots://add/today",
        ] {
            XCTAssertNil(
              try TeraRootTab.resolve(url: XCTUnwrap(URL(string: rawValue))),
              "Expected noncanonical deep link rejection: \(rawValue)"
            )
        }
    }

    func testDeepLinksRejectAuthorityDecorationsAndEncodedAliases() {
        for rawValue in [
          "radroots://user@today",
          "radroots://user:password@add",
          "radroots://today:7447",
          "radroots://today?",
          "radroots://today?source=widget",
          "radroots://today#",
          "radroots://add#composer",
          "radroots://%74oday",
          "radroots://%61dd",
          "radroots://today/%61dd",
        ] {
            if let url = URL(string: rawValue) {
                XCTAssertNil(
                  TeraRootTab.resolve(url: url),
                  "Expected decorated or encoded deep link rejection: \(rawValue)"
                )
            }
        }
    }
}
