import Foundation

@MainActor
final class TeraVisibilityStore: ObservableObject {
  @Published private(set) var policy: TeraAuthorVisibilityPolicy?
  @Published private(set) var isWorking = false
  @Published private(set) var message: String?
  var willChange: () -> Void = {}
  var didChange: () async -> Void = {}
  private let client: TeraRuntimeClient
  private var generation = TeraSessionGeneration.initial
  private var configuration: TeraPresentationConfiguration?
  private var mutationInProgress = false

  init(runtimeClient: TeraRuntimeClient) {
    client = runtimeClient
  }

  func configure(snapshot: TeraRuntimeSnapshot) {
    let next = TeraPresentationConfiguration(snapshot: snapshot)
    guard configuration != next else { return }
    generation = generation.invalidated()
    configuration = next
    policy = nil
    message = nil
    isWorking = mutationInProgress
  }

  func load() async {
    guard !mutationInProgress else { return }
    let requested = generation
    do {
      let result = try await client.authorVisibility()
      guard requested == generation, generation.isActive, !Task.isCancelled else { return }
      policy = result
      message = nil
    } catch {
      guard requested == generation, !Task.isCancelled else { return }
      policy = nil
      message = "Visibility preferences are unavailable. Retry after reopening the account if needed."
    }
  }

  func change(author: String, to visibility: TeraAuthorVisibility) async {
    guard !mutationInProgress, generation.isActive, !Task.isCancelled else { return }
    mutationInProgress = true
    isWorking = true
    generation = generation.invalidated()
    let requested = generation
    willChange()
    defer { mutationInProgress = false; isWorking = false }
    do {
      let result = try await client.setAuthorVisibility(author: author, visibility: visibility)
      guard requested == generation, generation.isActive, !Task.isCancelled else { return }
      policy = result
      message = visibility == .visible ? "Author restored to local results." : "Author hidden from local results."
      await didChange()
    } catch {
      guard requested == generation else { return }
      policy = nil
      message = "The change could not be confirmed. Content remains cleared. Reopen the account to read the saved preferences."
    }
  }

  func stop() {
    generation = generation.invalidated()
    policy = nil
    message = nil
  }
}
