import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraDiagnosticPolicyTests: XCTestCase {
  func testAllSinksReceiveOnlyClosedFieldsAndNoRawMessageCategoryOrValues() async throws {
    let capture = DiagnosticSink()
    let buffer = TeraDiagnosticsBuffer()
    let telemetry = TeraSanitizedTelemetry(sink: RadrootsMultiplexTelemetry([capture, buffer]))
    let canary = "ordinary private text 49.123456,-123.987654 Bearer CANARY_TOKEN /Users/private/file"
    let event = try RadrootsTelemetryEvent(
      name: "ios.lifecycle.operation_completed", category: "private_category", message: canary,
      fields: [
        .string("operation", "start"), .string("phase", "running"),
        .string("code", canary), .string("authorization", canary),
        .string("plain", canary), .integer("count", 7), .integer("id", 123_456_789),
        .stringList("list", [canary]), .double("latitude", 49.123456),
      ]
    )
    await telemetry.record(event)
    let observed = await capture.events
    let safe = try XCTUnwrap(observed.first)
    XCTAssertNil(safe.message)
    XCTAssertEqual(safe.category, "ios_lifecycle")
    XCTAssertEqual(safe.fields.map(\.key), ["operation", "phase", "count"])
    XCTAssertFalse(String(describing: safe).contains(canary))
    let records = await buffer.records()
    XCTAssertEqual(records.first?.fields, ["operation": "start", "phase": "running", "count": "7"])
    XCTAssertEqual(TeraDiagnosticPolicy.sanitized(safe), safe)
  }

  func testDirectBufferRejectsUnknownNamesKeysAndHighCardinalityValues() async throws {
    let buffer = TeraDiagnosticsBuffer(capacity: 16)
    for index in 0 ..< 40 {
      let event = try RadrootsTelemetryEvent(
        name: "private.post.\(index)", category: "private_location",
        message: "Raw private content", fields: [.string("unknown", "Bearer canary")]
      )
      await buffer.record(event)
    }
    let records = await buffer.records()
    XCTAssertEqual(records.count, 16)
    XCTAssertTrue(records.allSatisfy { $0.name == "ios.diagnostics.redacted" && $0.category == "ios_lifecycle" && $0.fields.isEmpty })
    let event = try RadrootsTelemetryEvent(name: "ios.lifecycle.active", fields: [
      .integer("count", Int64.max), .integer("schema", -1), .string("phase", "secret"),
    ])
    await buffer.record(event)
    let final = await buffer.records()
    XCTAssertEqual(final.last?.fields, [:])
  }

  func testMetadataUsesFiniteCodesAndBoundedNumericBuilds() {
    let canary = "public key or private content /Users/person/file"
    XCTAssertEqual(TeraDiagnosticPolicy.version(canary), "unavailable")
    XCTAssertEqual(TeraDiagnosticPolicy.version("0.1.0-alpha"), "0.1.0-alpha")
    XCTAssertEqual(TeraDiagnosticPolicy.build(canary), "unavailable")
    XCTAssertEqual(TeraDiagnosticPolicy.build(String(repeating: "1", count: 11)), "unavailable")
    XCTAssertEqual(TeraDiagnosticPolicy.build("1"), "1")
    XCTAssertEqual(TeraDiagnosticPolicy.code(canary, allowed: TeraDiagnosticPolicy.phases), "unavailable")
  }
}

private actor DiagnosticSink: RadrootsTelemetry {
  var events: [RadrootsTelemetryEvent] = []
  func record(_ event: RadrootsTelemetryEvent) {
    events.append(event)
  }
}
