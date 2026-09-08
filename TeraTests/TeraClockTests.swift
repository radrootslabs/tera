import Foundation
@testable import TeraApp
import XCTest

final class TeraClockTests: XCTestCase {
  func testUnixClockUsesCheckedFlooringAtBothPrecisions() throws {
    let clock = TeraClock(now: { Date(timeIntervalSince1970: 1_800_000_000.125) })

    XCTAssertEqual(try clock.unixSeconds(), 1_800_000_000)
    XCTAssertEqual(try clock.unixMilliseconds(), 1_800_000_000_125)
    XCTAssertEqual(try TeraClock.fixed(unixSeconds: 1).unixSeconds(requirePositive: true), 1)
  }

  func testUnixClockRejectsNonfinitePreEpochZeroAndOverflow() {
    let vectors: [(TimeInterval, TeraClockError)] = [
      (.nan, .nonfinite),
      (.infinity, .nonfinite),
      (-0.001, .beforeUnixEpoch),
      (TimeInterval.greatestFiniteMagnitude, .overflow),
    ]
    for (value, expected) in vectors {
      XCTAssertThrowsError(
        try TeraClock.unixMilliseconds(from: Date(timeIntervalSince1970: value))
      ) { error in
        XCTAssertEqual(error as? TeraClockError, expected)
      }
    }
    XCTAssertThrowsError(
      try TeraClock.unixSeconds(
        from: Date(timeIntervalSince1970: 0),
        requirePositive: true
      )
    ) { error in
      XCTAssertEqual(error as? TeraClockError, .zeroNotAllowed)
    }
    XCTAssertThrowsError(
      try TeraClock.signedUnixMilliseconds(
        from: Date(timeIntervalSince1970: TimeInterval(Int64.max) / 1000 + 1)
      )
    ) { error in
      XCTAssertEqual(error as? TeraClockError, .overflow)
    }
  }

  func testGenerationTransitionRejectsMaximumWithoutWrapping() throws {
    XCTAssertEqual(try TeraCheckedStateTransition.nextGeneration(after: 0), 1)
    XCTAssertEqual(try TeraCheckedStateTransition.nextGeneration(after: UInt64.max - 1), .max)
    XCTAssertThrowsError(try TeraCheckedStateTransition.nextGeneration(after: .max)) {
      error in
      XCTAssertEqual(error as? TeraStateTransitionError, .generationOverflow)
    }
  }
}
