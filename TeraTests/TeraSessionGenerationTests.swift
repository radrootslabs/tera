@testable import TeraApp
import XCTest

final class TeraSessionGenerationTests: XCTestCase {
  func testSessionAdvancesWithoutChangingExistingDiagnosticValues() throws {
    let first = try TeraSessionGeneration.initial.next()
    let second = try first.next()
    XCTAssertNotEqual(first, second)
    XCTAssertEqual(first.diagnosticValue, "1")
    XCTAssertEqual(second.diagnosticValue, "2")
    let identity = TeraRuntimeOperationIdentity(generation: first, sequence: 7, kind: .startup)
    XCTAssertEqual(identity.rawValue, "ios-runtime-1-7-startup")
    XCTAssertEqual(TeraProjectionRevision(rawValue: UInt64.max).rawValue, UInt64.max)
  }

  func testExhaustionInvalidatesCallbacksAndNeverReusesAnEarlierSession() throws {
    let beforeLast = TeraSessionGeneration(rawValue: UInt64.max - 1)
    let last = try beforeLast.next()
    XCTAssertEqual(last.diagnosticValue, String(UInt64.max))
    XCTAssertThrowsError(try last.next()) { error in
      XCTAssertEqual(error as? TeraStateTransitionError, .generationOverflow)
    }
    let exhausted = last.invalidated()
    XCTAssertNotEqual(exhausted, last)
    XCTAssertNotEqual(exhausted, .initial)
    XCTAssertFalse(exhausted.isActive)
    XCTAssertEqual(exhausted.invalidated(), exhausted)
    XCTAssertThrowsError(try exhausted.next())
    XCTAssertThrowsError(try exhausted.requireActive())
  }
}

extension TeraRuntimeClientTests {
    func testRestartChangesSessionButPreservesDurableLaunchScope() async throws {
        let harness = RuntimeHarness()
        let client = TeraRuntimeClient(factory: harness.start)
        let configuration = makeConfiguration(generation: "06")
        let first = try await client.start(configuration: configuration)
        let firstLifecycle = await client.lifecycle()
        _ = try await client.stop()
        let second = try await client.start(configuration: configuration)
        let secondLifecycle = await client.lifecycle()
        XCTAssertNotEqual(firstLifecycle, secondLifecycle)
        XCTAssertEqual(first.identity, second.identity)
        let launches = await harness.launchConfigurations()
        XCTAssertEqual(launches, [configuration, configuration])
        _ = try await client.stop()
    }
}
