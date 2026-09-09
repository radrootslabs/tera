@testable import TeraApp

enum TeraAddSchemaFixtures {
  static func schemas() -> [TeraAddSchema] {
    [update, photoUpdate, ask, event, foodAvailability]
  }

  private static let field = { @Sendable
    (
      id: String,
      label: String,
      kind: TeraAddFieldKind,
      required: Bool,
      choices: [String],
      maxBytes: UInt64?,
      maxItems: UInt16?
    ) in
    TeraAddField(
      schemaVersion: 1,
      id: id,
      label: label,
      kind: kind,
      required: required,
      choices: choices,
      maxBytes: maxBytes,
      maxItems: maxItems
    )
  }

  private static let text = { @Sendable
    (id: String, label: String, kind: TeraAddFieldKind, required: Bool, maximum: UInt64?) in
    field(id, label, kind, required, [], maximum, nil)
  }

  private static let media = { @Sendable (required: Bool, maximum: UInt16) in
    field("media", "Photos", .media, required, [], 10 * 1024 * 1024, maximum)
  }

  private static var update: TeraAddSchema {
    TeraAddSchema(
      schemaVersion: 1,
      commandType: .createUpdate,
      label: "Update",
      fields: [text("content", "Update", .multilineText, true, 65535)]
    )
  }

  private static var photoUpdate: TeraAddSchema {
    TeraAddSchema(
      schemaVersion: 1,
      commandType: .createPhotoUpdate,
      label: "Photo update",
      fields: [
        text("content", "Update", .multilineText, true, 65535),
        media(true, 20),
      ]
    )
  }

  private static var ask: TeraAddSchema {
    TeraAddSchema(
      schemaVersion: 1,
      commandType: .createAsk,
      label: "Ask",
      fields: [
        text("content", "Question", .multilineText, true, 65535),
        media(false, 20),
      ]
    )
  }

  private static var event: TeraAddSchema {
    TeraAddSchema(
      schemaVersion: 1,
      commandType: .createEvent,
      label: "Event",
      fields: [
        text("identifier", "Identifier", .text, true, 256),
        text("title", "Title", .text, true, 256),
        text("content", "Description", .multilineText, false, 65535),
        text("event_start", "Starts", .dateTime, true, nil),
        text("event_end", "Ends", .dateTime, false, nil),
        text("location", "Location", .location, false, 256),
        media(false, 1),
      ]
    )
  }

  private static var foodAvailability: TeraAddSchema {
    TeraAddSchema(
      schemaVersion: 1,
      commandType: .createFoodAvailability,
      label: "Food availability",
      fields: [
        text("identifier", "Identifier", .text, true, 256),
        text("title", "Food", .text, true, 256),
        text("summary", "Summary", .text, true, 256),
        text("content", "Details", .multilineText, true, 65535),
        text("location", "Pickup location", .location, true, 256),
        text("price_amount", "Price", .decimal, true, 64),
        field("currency", "Currency", .choice, true, [], 3, nil),
        field(
          "unit", "Unit", .choice, true,
          ["g", "kg", "lb", "oz", "each", "dozen", "bunch", "punnet", "bag", "basket"],
          nil, nil
        ),
        text("quantity", "Available quantity", .decimal, false, 64),
        media(false, 20),
      ]
    )
  }
}
