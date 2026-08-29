@testable import RadrootsApp
import XCTest

final class RadrootsRootShellTests: XCTestCase {
    func testRootInventoryIsExactlyTodayThenAdd() {
        XCTAssertEqual(RadrootsRootTab.allCases.map(\.rawValue), ["today", "add"])
    }

    func testRestorationFailsClosedToToday() {
        XCTAssertEqual(RadrootsRootTab.resolve(nil), .today)
        XCTAssertEqual(RadrootsRootTab.resolve("capture"), .today)
        XCTAssertEqual(RadrootsRootTab.resolve("activity"), .today)
        XCTAssertEqual(RadrootsRootTab.resolve("settings"), .today)
    }

    func testDeepLinksAcceptOnlyCanonicalRootAuthorities() throws {
        for (rawValue, expected) in [
          ("radroots://today", RadrootsRootTab.today),
          ("RADROOTS://TODAY", RadrootsRootTab.today),
          ("radroots://add", RadrootsRootTab.add),
          ("RadRoots://AdD", RadrootsRootTab.add),
        ] {
            XCTAssertEqual(
              try RadrootsRootTab.resolve(url: XCTUnwrap(URL(string: rawValue))),
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
              try RadrootsRootTab.resolve(url: XCTUnwrap(URL(string: rawValue))),
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
                  RadrootsRootTab.resolve(url: url),
                  "Expected decorated or encoded deep link rejection: \(rawValue)"
                )
            }
        }
    }
}
