import Foundation

@MainActor
final class TeraProductStores {
  let today: TeraTodayStore
  let add: TeraAddStore
  let search: TeraSearchStore
  let me: TeraMeStore
  let settings: TeraSettingsStore
  let media: TeraMediaStore
  private var generation = TeraSessionGeneration.initial
  private var startupTask: Task<Void, Never>?

  init(
    runtimeClient: TeraRuntimeClient,
    addMedia: (any TeraAddMediaHandling)? = nil
  ) {
    today = TeraTodayStore(runtimeClient: runtimeClient)
    add = TeraAddStore(runtimeClient: runtimeClient, media: addMedia)
    search = TeraSearchStore(runtimeClient: runtimeClient)
    me = TeraMeStore(runtimeClient: runtimeClient)
    settings = TeraSettingsStore(runtimeClient: runtimeClient)
    media = TeraMediaStore(runtimeClient: runtimeClient)
  }

  deinit {
    startupTask?.cancel()
  }

  func configure(snapshot: TeraRuntimeSnapshot) {
    let wasStarting = startupTask != nil
    cancelStartup()
    if wasStarting {
      today.stop()
      add.suspend()
    }
    today.configure(snapshot: snapshot)
    add.configure(snapshot: snapshot)
    search.stop()
    search.configure(context: today.selectedContext)
    me.stop()
    me.configure(context: today.selectedContext)
    settings.configure(snapshot: snapshot)
    media.configure(snapshot: snapshot)
  }

  func start() {
    guard startupTask == nil, generation.isActive, !Task.isCancelled else { return }
    generation = generation.invalidated()
    guard generation.isActive else { return }
    let requested = generation
    startupTask = Task { [weak self, today, add] in
      async let todayStart: Void = today.start()
      async let addStart: Void = add.start()
      _ = await (todayStart, addStart)
      self?.finishStartup(requested, cancelled: Task.isCancelled)
    }
  }

  func resume() async {
    guard !Task.isCancelled else { return }
    start()
    guard let task = startupTask else { return }
    await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
  }

  private func finishStartup(_ requested: TeraSessionGeneration, cancelled: Bool) {
    guard generation == requested else { return }
    startupTask = nil
    if cancelled {
      today.stop()
      add.suspend()
    }
  }

  func suspend() {
    cancelStartup()
    today.stop()
    add.suspend()
    search.stop()
    me.stop()
    settings.stop()
    media.reset()
  }

  func stop() {
    cancelStartup()
    today.stop()
    add.stop()
    search.stop()
    me.stop()
    settings.stop()
    media.reset()
  }

  private func cancelStartup() {
    generation = generation.invalidated()
    startupTask?.cancel()
    startupTask = nil
  }
}
