import Foundation
@testable import RadrootsApp
import XCTest

final class RadrootsClockTests: XCTestCase {
  func testUnixClockUsesCheckedFlooringAtBothPrecisions() throws {
    let clock = RadrootsClock(now: { Date(timeIntervalSince1970: 1_800_000_000.125) })

    XCTAssertEqual(try clock.unixSeconds(), 1_800_000_000)
    XCTAssertEqual(try clock.unixMilliseconds(), 1_800_000_000_125)
    XCTAssertEqual(try RadrootsClock.fixed(unixSeconds: 1).unixSeconds(requirePositive: true), 1)
  }

  func testUnixClockRejectsNonfinitePreEpochZeroAndOverflow() {
    let vectors: [(TimeInterval, RadrootsClockError)] = [
      (.nan, .nonfinite),
      (.infinity, .nonfinite),
      (-0.001, .beforeUnixEpoch),
      (TimeInterval.greatestFiniteMagnitude, .overflow),
    ]
    for (value, expected) in vectors {
      XCTAssertThrowsError(
        try RadrootsClock.unixMilliseconds(from: Date(timeIntervalSince1970: value))
      ) { error in
        XCTAssertEqual(error as? RadrootsClockError, expected)
      }
    }
    XCTAssertThrowsError(
      try RadrootsClock.unixSeconds(
        from: Date(timeIntervalSince1970: 0),
        requirePositive: true
      )
    ) { error in
      XCTAssertEqual(error as? RadrootsClockError, .zeroNotAllowed)
    }
    XCTAssertThrowsError(
      try RadrootsClock.signedUnixMilliseconds(
        from: Date(timeIntervalSince1970: TimeInterval(Int64.max) / 1000 + 1)
      )
    ) { error in
      XCTAssertEqual(error as? RadrootsClockError, .overflow)
    }
  }

  func testGenerationTransitionRejectsMaximumWithoutWrapping() throws {
    XCTAssertEqual(try RadrootsCheckedStateTransition.nextGeneration(after: 0), 1)
    XCTAssertEqual(try RadrootsCheckedStateTransition.nextGeneration(after: UInt64.max - 1), .max)
    XCTAssertThrowsError(try RadrootsCheckedStateTransition.nextGeneration(after: .max)) {
      error in
      XCTAssertEqual(error as? RadrootsStateTransitionError, .generationOverflow)
    }
  }
}
