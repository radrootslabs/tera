import Foundation

enum TeraAddPresentation {
  static func sorted(_ drafts: [TeraDraftStatus]) -> [TeraDraftStatus] {
    drafts.sorted {
      if $0.updatedAtUnixMilliseconds == $1.updatedAtUnixMilliseconds {
        return $0.id < $1.id
      }
      return $0.updatedAtUnixMilliseconds > $1.updatedAtUnixMilliseconds
    }
  }

  static func newForm(
    type: TeraAddCommandType,
    identifier: @Sendable () -> String,
    clock: TeraClock,
    timeZone: TimeZone = .current
  ) -> TeraAddForm {
    var form = TeraAddForm.empty(type)
    if type == .createEvent || type == .createFoodAvailability {
      let value = identifier()
      if isValidIdentifier(value) {
        form.identifier = value
      }
    }
    if type == .createEvent {
      guard let now = try? clock.unixSeconds() else {
        return form
      }
      TeraCalendarEditing.initialize(&form, now: now, timeZone: timeZone)
    }
    return form
  }

  static func isValidIdentifier(_ value: String) -> Bool {
    value.utf8.count == 32
      && value.utf8.allSatisfy { byte in
        (byte >= 0x30 && byte <= 0x39) || (byte >= 0x61 && byte <= 0x66)
      }
  }

  static func message(for error: Error) -> String {
    TeraUserMessages.text(for: error, fallback: .addOperationFailed)
  }

  static func failure(for error: Error) -> TeraRuntimeFailure? {
    if case let TeraRuntimeClientError.add(failure) = error {
      return failure
    }
    return error as? TeraRuntimeFailure
  }
}
