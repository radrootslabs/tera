extension TeraAddStore {
  func retry(_ draft: TeraDraftStatus? = nil) async {
    guard let draft = draft ?? activeDraft else { return }
    await retry(id: draft.id)
  }

  func cancel(_ draft: TeraDraftStatus? = nil) async {
    guard let draft = draft ?? activeDraft, draft.state.canCancel else { return }
    await cancel(id: draft.id)
  }
}
