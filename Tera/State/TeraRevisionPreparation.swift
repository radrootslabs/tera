import Foundation

/// Retain the action and exact editing snapshot before the first suspension.
/// A failed/abandoned waiter retries this capture; only explicit New resets it.
@MainActor
final class TeraRevisionPreparation {
  private struct Capture {
    let id: String
    let target: TeraRevisionTarget
    let form: TeraAddForm
  }

  private let client: TeraRuntimeClient
  private let media: (any TeraAddMediaHandling)?
  private var capture: Capture?

  init(client: TeraRuntimeClient, media: (any TeraAddMediaHandling)?) {
    self.client = client
    self.media = media
  }

  var isCaptured: Bool {
    capture != nil
  }

  func reset() {
    capture = nil
  }

  func prepare(
    target: TeraRevisionTarget,
    form: TeraAddForm,
    identifier: () -> String,
    ensureCurrent: () throws -> Void
  ) async throws -> TeraRevisionStatus {
    if capture == nil {
      let id = identifier()
      guard TeraAddPresentation.isValidIdentifier(id) else { throw TeraComposerAcknowledgment.unconfirmed }
      capture = Capture(id: id, target: target, form: form)
    }
    guard let capture, capture.target == target else { throw TeraComposerAcknowledgment.unconfirmed }
    let opened = try await TeraOpenedMedia.open(capture.form.media, using: media)
    defer { opened.close() }
    try ensureCurrent()
    return try await client.saveRevisionIntent(requestID: capture.id, target: capture.target,
                                               replacement: TeraAddRuntimeInput(form: capture.form, media: opened.handles))
  }
}
