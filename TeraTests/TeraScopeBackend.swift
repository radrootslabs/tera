import Foundation
@testable import TeraApp

actor TeraScopeBackend: TeraRuntimeBackend {
  enum Call: Hashable { case snapshot, drafts, save, probe, page, reconcile, refresh, search, me, subscribe, media, invalidate }
  struct Pending {
    let pause: ResourceTestPause
    let failure: TeraRuntimeFailure?
  }

  private(set) var value = TeraScopeFixtures.snapshot()
  private var pending: [Call: [Pending]] = [:]
  private(set) var counts: [Call: Int] = [:]
  private var receivers: [@Sendable (TeraRuntimeChange) async -> Void] = []
  private(set) var tokens: [ResourceTestToken] = []
  private(set) var lastMeContext: TeraLocalNetwork?
  private var drafts: [TeraDraftStatus] = [TeraScopeFixtures.draft("old")]
  private var media: TeraVerifiedMediaArtifact
  private var pages: [String: TeraTodayPage] = [:]
  private var meCards = [TeraScopeFixtures.card("old")]
  private var revision: UInt64 = 0
  private var syncReceipt: TeraTodaySyncReceipt?
  private var reconciliationCards: [TeraTodayCard]?
  private var reconciliationGeneration: UInt64 = 1
  private(set) var reconciliationRequests: [TeraTodayReconcileRequest] = []

  init() throws {
    media = try TeraScopeFixtures.artifact("a")
  }

  func pause(_ call: Call, fails: Bool = false, failure: TeraRuntimeFailure? = nil) -> ResourceTestPause {
    let pause = ResourceTestPause()
    pending[call, default: []].append(Pending(pause: pause, failure: failure ?? (fails ? TeraScopeFixtures.failure() : nil)))
    return pause
  }

  func configure(_ value: TeraRuntimeSnapshot) {
    self.value = value
  }

  func setSyncReceipt(_ receipt: TeraTodaySyncReceipt) {
    syncReceipt = receipt
  }

  func setDrafts(_ values: [TeraDraftStatus]) {
    drafts = values
  }

  func setMedia(_ value: TeraVerifiedMediaArtifact) {
    media = value
  }

  func setPage(_ value: TeraTodayPage, cursor: String = "first") {
    pages[cursor] = value
  }

  func setMeCards(_ values: [TeraTodayCard]) {
    meCards = values
  }

  func setReconciliation(_ cards: [TeraTodayCard], generation: UInt64) {
    reconciliationCards = cards
    reconciliationGeneration = generation
  }

  func reconcileToday(request: TeraTodayReconcileRequest) async throws -> TeraTodayPage {
    reconciliationRequests.append(request)
    let saved = pages.isEmpty ? [TeraScopeFixtures.card(request.context.relayURLs.first ?? "none")] : pages.values.flatMap(\.items)
    let values = reconciliationCards ?? saved
    var seen = Set<String>()
    let items = values.filter { request.cardIDs.contains($0.id) && seen.insert($0.id).inserted }
    let generation = reconciliationGeneration
    try await wait(.reconcile)
    if let expected = request.expectedGeneration, expected != generation {
      throw TeraRuntimeFailure.local(operation: "test", code: "today_cursor_invalid", safeMessage: "Changed")
    }
    return TeraTodayPage(
      asOfUnixSeconds: request.asOfUnixSeconds, items: items, nextCursor: nil,
      projectionGeneration: generation
    )
  }

  private func wait(_ call: Call) async throws {
    counts[call, default: 0] += 1
    guard var queue = pending[call], !queue.isEmpty else { return }
    let next = queue.removeFirst()
    pending[call] = queue
    await next.pause.wait()
    if let failure = next.failure {
      throw failure
    }
  }

  func snapshot() async throws -> TeraRuntimeSnapshot {
    let snapshot = value
    try await wait(.snapshot)
    return snapshot
  }

  func addSchemas() -> [TeraAddSchema] {
    TeraAddSchemaFixtures.schemas()
  }

  func draftHeads(limit _: UInt16) async throws -> [TeraDraftStatus] {
    let result = drafts
    try await wait(.drafts)
    return result
  }

  func saveAddIntent(
    input: TeraAddRuntimeInput, existingDraftID _: String?, expectedRevision: UInt64?
  ) async throws -> TeraDraftStatus {
    let result = TeraScopeFixtures.draft(input.form.content, revision: (expectedRevision ?? 0) + 1)
    try await wait(.save)
    drafts = [result]
    return result
  }

  func probeBlossom() async throws -> TeraBlossomEvidence {
    let fingerprint = value.blossomConfiguration?.configFingerprint ?? ""
    try await wait(.probe)
    return TeraBlossomEvidence(
      schemaVersion: 2, origin: "http://127.0.0.1:3000", configFingerprint: fingerprint,
      state: "reachable", lastSuccessfulState: "probe", transportSecurity: "loopback_plaintext",
      observedAtUnixMilliseconds: 1, httpStatus: 200, errorCode: nil, serverErrorCode: nil,
      errorPhase: nil, retryable: false, possibleOrphan: false, attempts: 1
    )
  }

  func todayPage(request: TeraTodayPageRequest) async throws -> TeraTodayPage {
    let result = pages[request.cursor ?? "first"] ?? TeraTodayPage(
      asOfUnixSeconds: 1, items: [TeraScopeFixtures.card(request.context.relayURLs.first ?? "none")], nextCursor: nil
    )
    try await wait(.page)
    return result
  }

  private(set) var lastBackfillCursor: String?

  func refreshToday(
    context _: TeraLocalNetwork, nowUnixSeconds _: UInt64, update: TeraTodayProjectionUpdate,
    backfillCursor: String?
  ) async throws -> TeraTodaySyncReceipt {
    lastBackfillCursor = backfillCursor
    let result = syncReceipt ?? TeraTodaySyncFixtures.receipt(update: update)
    try await wait(.refresh)
    return result
  }

  func search(
    context _: TeraLocalNetwork, query: String, limit _: UInt16, asOfUnixSeconds _: UInt64
  ) async throws -> [TeraSearchResult] {
    try await wait(.search)
    return [TeraSearchResult(type: .card, id: query, card: TeraScopeFixtures.card(query), profile: nil)]
  }

  func me(context: TeraLocalNetwork, asOfUnixSeconds _: UInt64) async throws -> TeraMeSnapshot {
    lastMeContext = context
    let result = TeraMeSnapshot(publicKey: value.identity.publicKeyHex, profile: nil, cards: meCards)
    try await wait(.me)
    return result
  }

  func retrieveMedia(context _: TeraLocalNetwork, reference _: TeraMediaReference) async throws -> TeraVerifiedMediaArtifact {
    let result = media
    try await wait(.media)
    return result
  }

  func invalidateMediaArtifact(context _: TeraLocalNetwork, artifactID _: String) async throws -> Bool {
    try await wait(.invalidate)
    return true
  }

  func subscribe(
    bufferCapacity _: Int, receive: @escaping @Sendable (TeraRuntimeChange) async -> Void
  ) async throws -> any TeraRuntimeSubscriptionToken {
    let token = ResourceTestToken()
    receivers.append(receive)
    tokens.append(token)
    try await wait(.subscribe)
    return token
  }

  func emit(
    _ kind: TeraRuntimeChangeKind,
    delivery: TeraRuntimeChangeDelivery = .change,
    context: TeraLocalNetwork? = nil,
    exhausted: Bool = false
  ) async {
    revision += 1
    let emittedRevision = revision
    for receive in receivers {
      await receive(TeraRuntimeChange(
        schemaVersion: 3,
        scope: TeraRuntimeChangeScope(publicKey: String(repeating: "a", count: 64), sourceGeneration: String(repeating: "a", count: 64), context: context),
        epoch: String(repeating: "1", count: 32),
        revision: TeraProjectionRevision(rawValue: exhausted ? nil : emittedRevision),
        delivery: delivery, kind: kind, entityID: nil
      ))
    }
  }

  func shutdown() -> TeraRuntimeShutdownReceipt {
    TeraRuntimeShutdownReceipt(state: "closed", alreadyClosed: false)
  }
}
