import Foundation

/// Read-only capability observation and explicit intake, scoped to one account.
@MainActor
final class TeraMediaAccessStore {
  private(set) var support = TeraAddMediaSupport.unavailable {
    didSet { changed() }
  }

  var changed: () -> Void = {}
  private let media: (any TeraAddMediaHandling)?
  private var generation = TeraSessionGeneration.initial

  init(media: (any TeraAddMediaHandling)?) {
    self.media = media
  }

  func accept(_ support: TeraAddMediaSupport) {
    self.support = support
  }

  func invalidate() {
    generation = generation.invalidated()
  }

  func inspect() async throws -> TeraAddMediaSupport {
    try await media?.support() ?? .unavailable
  }

  func prepare(limit: Int, capture: Bool) async throws -> [TeraPreparedMedia] {
    guard limit > 0, let media else { throw TeraComposerAcknowledgment.unconfirmed }
    if capture {
      return try await [self.capture()]
    }
    let requested = generation
    let result = try await media.importImages(limit: limit)
    try ensureCurrent(requested)
    return result
  }

  func recheck() async {
    invalidate()
    let requested = generation
    let result = try? await media?.support()
    guard requested == generation, generation.isActive, !Task.isCancelled else { return }
    support = result ?? .unavailable
  }

  func capture() async throws -> TeraPreparedMedia {
    let requested = generation
    guard let media else { throw TeraComposerAcknowledgment.unconfirmed }
    let capabilities = try await media.support()
    try ensureCurrent(requested)
    support = capabilities
    guard support.camera else {
      throw TeraRuntimeFailure.local(operation: "add.media.camera", code: "ios.camera.unavailable",
                                     safeMessage: support.cameraAccess.guidance)
    }
    do {
      let result = try await media.captureImage()
      try ensureCurrent(requested)
      return result
    } catch {
      let refreshed = try? await media.support()
      try ensureCurrent(requested)
      support = refreshed ?? .unavailable
      throw error
    }
  }

  private func ensureCurrent(_ requested: TeraSessionGeneration) throws {
    guard generation == requested, generation.isActive, !Task.isCancelled else { throw CancellationError() }
  }
}
