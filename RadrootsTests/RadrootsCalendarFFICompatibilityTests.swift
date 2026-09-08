import Foundation
import RadrootsKitBindings
import XCTest

final class RadrootsCalendarFFICompatibilityTests: XCTestCase {
  private let publicKey = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"

  func testBothCalendarVariantsReopenThroughInstalledFFI() async throws {
    let root = FileManager.default.temporaryDirectory
      .appendingPathComponent("tera-calendar-ffi-\(UUID().uuidString.lowercased())")
    try FileManager.default.createDirectory(
      at: root.appendingPathComponent("radroots/users/\(publicKey)"),
      withIntermediateDirectories: true
    )
    defer { try? FileManager.default.removeItem(at: root) }
    let first = try await runtime(root)
    var queued = [FfiDraftStatusRecord]()
    for (index, timing) in [FfiEventTimingKind.allDay, .timed].enumerated() {
      let input = calendar(timing)
      let saved = try await first.phase1SaveDraft(
        draftId: String(repeating: index == 0 ? "01" : "02", count: 16),
        input: input,
        authoredAtUnixS: 1_900_000_000,
        expectedRevision: nil,
        persistedAtUnixMs: 1_900_000_000_000
      )
      let form = try XCTUnwrap(saved.form)
      XCTAssertEqual(form.commandType, .createEvent)
      XCTAssertEqual(form.eventTiming, timing)
      XCTAssertEqual(form.eventStartDate, input.eventStartDate)
      XCTAssertEqual(form.eventEndDate, input.eventEndDate)
      XCTAssertEqual(form.eventStartUnixS, input.eventStartUnixS)
      XCTAssertEqual(form.eventEndUnixS, input.eventEndUnixS)
      XCTAssertEqual(form.eventTimezone, input.eventTimezone)
      let status = try await first.phase1QueueDraft(
        draftId: saved.draftId,
        expectedRevision: saved.revision,
        policy: FfiQueuePolicyRecord(
          schemaVersion: 1,
          relayUrls: ["wss://relay.example"],
          satisfaction: .allAccepted,
          deliveryDeadlineUnixMs: 2_000_000_000_000,
          cancellation: .localCooperative
        ),
        queuedAtUnixMs: 1_900_000_000_001
      )
      XCTAssertEqual(status.state, .queued)
      XCTAssertNotNil(status.operationId)
      XCTAssertEqual(status.form, saved.form)
      queued.append(status)
    }
    _ = try await first.shutdown()
    let reopened = try await runtime(root)
    for expected in queued {
      let restored = try await reopened.phase1DraftStatus(draftId: expected.draftId)
      XCTAssertEqual(restored, expected)
    }
    _ = try await reopened.shutdown()
  }

  private func runtime(_ root: URL) async throws -> RadrootsRuntime {
    try await RadrootsRuntime(
      applicationSupportDirectory: root.path,
      publicKeyHex: publicKey,
      sourceGenerationHex: String(repeating: "04", count: 32),
      sourceGenerationCreatedAtUnixMs: 1_800_000_000_000,
      protectedData: .available
    )
  }

  private func calendar(_ timing: FfiEventTimingKind) -> FfiAddDraftInput {
    let allDay = timing == .allDay
    return FfiAddDraftInput(
      schemaVersion: 1,
      commandType: .createEvent,
      content: "Synthetic calendar boundary fixture",
      identifier: allDay ? "all-day-fixture" : "timed-fixture",
      title: "Synthetic calendar event",
      summary: nil,
      location: "Fixture location",
      eventTiming: timing,
      eventStartDate: allDay ? "2030-03-18" : nil,
      eventEndDate: allDay ? "2030-03-19" : nil,
      eventStartUnixS: allDay ? nil : 1_900_003_600,
      eventEndUnixS: allDay ? nil : 1_900_007_200,
      eventTimezone: allDay ? nil : "Etc/UTC",
      priceAmount: nil,
      currency: nil,
      unit: nil,
      quantity: nil,
      foodPublishedAtUnixS: nil,
      foodStatus: nil,
      media: []
    )
  }
}
