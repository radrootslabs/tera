import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraCalendarComposerFFITests: XCTestCase {
  func testNativeComposerInputsUseTheActualGeneratedSharedPlanAndReopenExactly() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let first = try await fixture.runtime()
    var saved = [FfiDraftStatusRecord]()
    for (index, mode) in [TeraEventTiming.allDay, .timed].enumerated() {
      let form = try form(mode)
      let input = TeraAddRuntimeInput(form: form, media: []).generatedValue
      let draft = try await first.phase1SaveDraft(draftId: String(repeating: index == 0 ? "51" : "52", count: 16),
                                                  input: input, authoredAtUnixS: 1_900_000_000, expectedRevision: nil,
                                                  persistedAtUnixMs: 1_900_000_000_000)
      XCTAssertEqual(draft.form?.eventStartDate, input.eventStartDate)
      XCTAssertEqual(draft.form?.eventEndDate, input.eventEndDate)
      XCTAssertEqual(input.eventStartDate, "2026-09-05")
      XCTAssertEqual(draft.form?.eventStartUnixS, form.eventStartUnixSeconds)
      XCTAssertEqual(draft.form?.eventEndUnixS, form.eventEndUnixSeconds)
      XCTAssertEqual(draft.form?.eventTimezone, form.eventTimezone)
      let queued = try await first.phase1QueueDraft(draftId: draft.draftId, expectedRevision: draft.revision,
                                                    policy: FfiQueuePolicyRecord(schemaVersion: 1, relayUrls: ["wss://relay.example"],
                                                                                 satisfaction: .allAccepted, deliveryDeadlineUnixMs: 2_000_000_000_000,
                                                                                 cancellation: .localCooperative), queuedAtUnixMs: 1_900_000_000_001)
      XCTAssertEqual(queued.state, .queued)
      XCTAssertEqual(queued.form, draft.form)
      XCTAssertNotNil(queued.operationId)
      saved.append(queued)
    }
    _ = try await first.shutdown()
    let reopened = try await fixture.runtime()
    for expected in saved {
      let actual = try await reopened.phase1DraftStatus(draftId: expected.draftId)
      XCTAssertEqual(actual, expected)
    }
    _ = try await reopened.shutdown()
  }

  func testActualSharedBoundaryRejectsPartialDatesExclusiveRangesAndInvalidZones() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    var partial = try form(.allDay)
    partial.eventStartDate = "2026-09-"
    var reversed = try form(.allDay)
    reversed.eventEndDate = reversed.eventStartDate
    var timed = try form(.timed)
    timed.eventEndUnixSeconds = timed.eventStartUnixSeconds
    var zone = try form(.timed)
    zone.eventTimezone = "Mars/Olympus"
    let codes = ["invalid_event_start_date", "invalid_event_range", "invalid_event_range", "invalid_event_timezone"]
    for (index, form) in [partial, reversed, timed, zone].enumerated() {
      do {
        _ = try await runtime.phase1SaveDraft(draftId: String(repeating: String(index + 3), count: 32),
                                              input: TeraAddRuntimeInput(form: form, media: []).generatedValue,
                                              authoredAtUnixS: 1_900_000_000, expectedRevision: nil,
                                              persistedAtUnixMs: 1_900_000_000_000)
        XCTFail("Invalid calendar input must fail strict shared planning before saving a draft.")
      } catch let TeraAppError.Failure(report) {
        XCTAssertEqual(report.code, codes[index])
      }
    }
    let drafts = try await runtime.phase1DraftHeads(limit: 10)
    XCTAssertTrue(drafts.isEmpty)
    _ = try await runtime.shutdown()
  }

  private func form(_ mode: TeraEventTiming) throws -> TeraAddForm {
    let zone = try XCTUnwrap(TimeZone(identifier: "America/Vancouver"))
    var form = TeraAddPresentation.newForm(type: .createEvent, identifier: { String(repeating: "a", count: 32) },
                                           clock: .fixed(unixSeconds: 1_788_568_200), timeZone: zone)
    form.eventTiming = mode
    form.title = "Synthetic harvest"
    form.content = "Synthetic calendar composer"
    form.eventStartDate = TeraCivilDateInput(raw: "2026-09-").replacing(2, with: "5")
    form.eventEndDate = "2026-09-07"
    return form
  }
}
