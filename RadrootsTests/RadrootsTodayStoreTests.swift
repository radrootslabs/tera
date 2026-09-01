@testable import RadrootsApp
import XCTest

final class RadrootsTodayStoreTests: XCTestCase {
    @MainActor
    func testFrozenPaginationPreservesEqualTimestampCardsWithoutDuplicates() async throws {
        let context = makeContext(id: "one", label: "One")
        let first = makeCard(id: "a", type: .update, authoredAt: 1_800_000_000)
        let second = makeCard(id: "b", type: .ask, authoredAt: 1_800_000_000)
        let backend = TodayBackend(
            pages: [
              "one:first": RadrootsTodayPage(
                asOfUnixSeconds: 1_800_000_100,
                items: [first],
                nextCursor: "same-time"
              ),
              "one:same-time": RadrootsTodayPage(
                asOfUnixSeconds: 1_800_000_100,
                items: [first, second],
                nextCursor: nil
              ),
            ]
        )
        let client = try await startedClient(backend: backend)
        let store = RadrootsTodayStore(
          runtimeClient: client,
          contexts: [context],
          pageSize: 1,
          clock: .fixed(unixSeconds: 1_800_000_100)
        )

        await store.reload(refreshProjection: false)
        XCTAssertEqual(store.cards.map(\.id), ["a"])
        XCTAssertTrue(store.canLoadNextPage)
        await store.loadNextPage()

        XCTAssertEqual(store.cards.map(\.id), ["a", "b"])
        XCTAssertEqual(store.state, .loaded)
        XCTAssertFalse(store.canLoadNextPage)
        _ = try await client.stop()
    }

    @MainActor
    func testContextSwitchRejectsLateCancelledContextResult() async throws {
        let firstContext = makeContext(id: "slow", label: "Slow")
        let secondContext = makeContext(id: "current", label: "Current")
        let stale = makeCard(id: "stale", type: .update)
        let current = makeCard(id: "current", type: .photoUpdate)
        let backend = TodayBackend(
          pages: [
            "slow:first": RadrootsTodayPage(
              asOfUnixSeconds: 1_800_000_100,
              items: [stale],
              nextCursor: nil
            ),
            "current:first": RadrootsTodayPage(
              asOfUnixSeconds: 1_800_000_100,
              items: [current],
              nextCursor: nil
            ),
          ],
          delays: ["slow": 80_000_000]
        )
        let client = try await startedClient(backend: backend)
        let store = RadrootsTodayStore(
          runtimeClient: client,
          contexts: [firstContext, secondContext],
          clock: .fixed(unixSeconds: 1_800_000_100)
        )

        let staleRequest = Task { await store.reload(refreshProjection: false) }
        try await Task.sleep(nanoseconds: 2_000_000)
        store.selectContext(id: "current")
        await staleRequest.value
        try await Task.sleep(nanoseconds: 20_000_000)

        XCTAssertEqual(store.selectedContextID, "current")
        XCTAssertEqual(store.cards.map(\.id), ["current"])
        _ = try await client.stop()
    }

    @MainActor
    func testFiveCardsExposeProfileThreadMediaOverlayAndAccessibleProductFields() {
        let profile = RadrootsProfileSummary(
          authorPublicKey: String(repeating: "a", count: 64),
          name: "fieldnotes",
          displayName: "Field Notes Farm",
          about: nil,
          picture: nil,
          banner: nil,
          nip05: "farm@example.com",
          website: nil,
          lightningAddress: nil
        )
        let failedMedia = RadrootsMediaReference(
          referenceFingerprint: String(repeating: "d", count: 64),
          url: "https://blossom.example/\(String(repeating: "b", count: 64))",
          sha256: String(repeating: "b", count: 64),
          mediaType: "image/jpeg",
          width: 1200,
          height: 800,
          byteSize: 400,
          alt: "Carrots in a basket",
          verification: .failed
        )
        let thread = RadrootsThreadEntry(
          id: "reply",
          authorPublicKey: String(repeating: "c", count: 64),
          content: "Are these available Saturday?",
          authoredAtUnixSeconds: 1_800_000_001,
          type: .reply,
          root: "root",
          parentEventID: "root",
          authorProfile: nil
        )
        let cards = RadrootsTodayCardType.allCases.map { type in
            makeCard(
              id: type.rawValue,
              type: type,
              profile: profile,
              media: type == .photoUpdate ? [failedMedia] : [],
              thread: type == .ask ? [thread] : [],
              localState: type == .foodAvailability ? "partially_delivered" : nil
            )
        }

        XCTAssertEqual(cards.map(\.type), RadrootsTodayCardType.allCases)
        XCTAssertEqual(cards[1].media.first?.verifiedArtifactID, nil)
        XCTAssertEqual(cards[2].thread.first?.type, .reply)
        XCTAssertEqual(cards[4].priceSummary, "3 CAD/lb")
        XCTAssertEqual(cards[4].quantity, "12")
        XCTAssertTrue(cards[4].accessibilitySummary.contains("Food availability"))
        XCTAssertTrue(cards[4].accessibilitySummary.contains("Field Notes Farm"))
        XCTAssertTrue(cards[4].accessibilitySummary.contains("partially_delivered"))
    }

    @MainActor
    func testOfflineRefreshStillLoadsTheDurableCachedFeed() async throws {
        let context = makeContext(id: "offline", label: "Offline")
        let cached = makeCard(id: "cached", type: .update)
        let failure = RadrootsRuntimeFailure(
          schemaVersion: 1,
          code: "today_relay_offline",
          category: "relay",
          retryable: true,
          recoveryActions: ["retry"],
          operationID: "test.today.refresh",
          capabilityID: "nostr_source",
          safeMessage: "Saved posts are available offline."
        )
        let backend = TodayBackend(
          pages: [
            "offline:first": RadrootsTodayPage(
              asOfUnixSeconds: 1_800_000_100,
              items: [cached],
              nextCursor: "cached-next"
            ),
            "offline:cached-next": RadrootsTodayPage(
              asOfUnixSeconds: 1_800_000_100,
              items: [makeCard(id: "cached-next", type: .ask)],
              nextCursor: nil
            ),
          ],
          refreshFailure: failure
        )
        let client = try await startedClient(backend: backend)
        let store = RadrootsTodayStore(
          runtimeClient: client,
          contexts: [context],
          clock: .fixed(unixSeconds: 1_800_000_100)
        )

        await store.reload()

        XCTAssertEqual(store.cards.map(\.id), ["cached"])
        XCTAssertEqual(store.state, .offline(message: RadrootsUserMessages.text(.todayUnavailable)))
        await store.loadNextPage()
        XCTAssertEqual(store.cards.map(\.id), ["cached", "cached-next"])
        XCTAssertEqual(store.state, .offline(message: RadrootsUserMessages.text(.todayUnavailable)))
        _ = try await client.stop()
    }

    @MainActor
    private func startedClient(backend: TodayBackend) async throws -> RadrootsRuntimeClient {
        let client = RadrootsRuntimeClient { _ in
            await RadrootsRuntimeBackendStart(backend: backend, snapshot: backend.snapshot())
        }
        _ = try await client.start(configuration: makeConfiguration())
        return client
    }

    private func makeConfiguration() -> RadrootsRuntimeLaunchConfiguration {
        RadrootsRuntimeLaunchConfiguration(
          applicationSupportDirectory: "/tmp/radroots-today-tests",
          publicKeyHex: String(repeating: "ab", count: 32),
          sourceGenerationHex: String(repeating: "cd", count: 32),
          sourceGenerationCreatedAtUnixMilliseconds: 1,
          protectedData: .available,
          networkProfile: .simulator,
          writableRelays: ["ws://127.0.0.1:7447"],
          blossom: RadrootsBlossomEndpointConfiguration(
            hostKind: .simulator,
            endpointAuthority: .loopbackDevelopment,
            primaryOrigin: "http://127.0.0.1:3000",
            fallbackOrigins: []
          ),
          app: RadrootsRuntimeAppMetadata(
            bundleIdentifier: "org.radroots.today-tests",
            version: "0.1.0-alpha",
            buildNumber: "1",
            buildSHA: nil
          ),
          signerGeneration: "today-tests",
          signer: TodayTestSigner(),
          adoptBootstrapSettings: false
        )
    }
}

private func makeContext(id: String, label: String) -> RadrootsLocalNetwork {
    RadrootsLocalNetwork(
      schemaVersion: 1,
      id: id,
      label: label,
      relayURLs: ["ws://127.0.0.1:7447"],
      locality: nil,
      followedAuthors: [],
      generation: 1
    )
}

private func makeCard(
  id: String,
  type: RadrootsTodayCardType,
  authoredAt: UInt64 = 1_800_000_000,
  profile: RadrootsProfileSummary? = nil,
  media: [RadrootsMediaReference] = [],
  thread: [RadrootsThreadEntry] = [],
  localState: String? = nil
) -> RadrootsTodayCard {
    RadrootsTodayCard(
      id: id,
      type: type,
      sourceEventID: "event-\(id)",
      sourceAddress: type == .event || type == .foodAvailability ? "address-\(id)" : nil,
      authorPublicKey: String(repeating: "a", count: 64),
      contractID: "contract-\(type.rawValue)",
      title: type == .event ? "Saturday market" : type == .foodAvailability ? "Carrots" : nil,
      content: type == .ask ? "Who has seedlings?" : "Fresh from the field",
      authoredAtUnixSeconds: authoredAt,
      effectiveAtUnixSeconds: authoredAt,
      eventStartUnixSeconds: type == .event ? authoredAt + 3600 : nil,
      eventEndUnixSeconds: nil,
      location: type == .event || type == .foodAvailability ? "Town square" : nil,
      priceAmount: type == .foodAvailability ? "3" : nil,
      priceCurrency: type == .foodAvailability ? "CAD" : nil,
      priceUnit: type == .foodAvailability ? "lb" : nil,
      quantity: type == .foodAvailability ? "12" : nil,
      foodSummary: type == .foodAvailability ? "Fresh carrots" : nil,
      foodPublishedAtUnixSeconds: type == .foodAvailability ? authoredAt : nil,
      foodStatus: type == .foodAvailability ? "active" : nil,
      contextRank: 1,
      inclusionReason: "locality_missing_fallback",
      media: media,
      lifecycle: .active,
      rankDigest: "rank-\(id)",
      authorProfile: profile,
      thread: thread,
      localOperationID: localState == nil ? nil : "operation-\(id)",
      localOperationState: localState
    )
}

private struct TodayTestSigner: RadrootsRuntimeSigner {
    func availability() async -> RadrootsRuntimeSignerAvailability {
        .ready
    }

    func sign(_: RadrootsRuntimeSigningRequest) async -> RadrootsRuntimeSigningOutcome {
        .failed
    }
}

private actor TodayBackend: RadrootsRuntimeBackend {
    private let pages: [String: RadrootsTodayPage]
    private let delays: [String: UInt64]
    private let refreshFailure: RadrootsRuntimeFailure?
    private var closed = false

    init(
      pages: [String: RadrootsTodayPage],
      delays: [String: UInt64] = [:],
      refreshFailure: RadrootsRuntimeFailure? = nil
    ) {
        self.pages = pages
        self.delays = delays
        self.refreshFailure = refreshFailure
    }

    func snapshot() -> RadrootsRuntimeSnapshot {
        RadrootsRuntimeSnapshot(
          identity: RadrootsRuntimeIdentity(
            publicKeyHex: String(repeating: "ab", count: 32),
            hostSignerConfigured: true
          ),
          relay: nil,
          blossomConfiguration: nil,
          blossomEvidence: nil,
          crateName: "radroots_mobile_ffi",
          crateVersion: "0.1.0-alpha",
          isClosed: closed
        )
    }

    func todayPage(request: RadrootsTodayPageRequest) async throws -> RadrootsTodayPage {
        if let delay = delays[request.context.id] {
            try await Task.sleep(nanoseconds: delay)
        }
        let key = "\(request.context.id):\(request.cursor ?? "first")"
        guard let page = pages[key] else {
            throw RadrootsRuntimeFailure.local(
              operation: "test.today.page",
              code: "test.page_missing",
              safeMessage: "The requested test page is missing."
            )
        }
        return page
    }

    func refreshToday(
      context _: RadrootsLocalNetwork,
      nowUnixSeconds _: UInt64,
      update: RadrootsTodayProjectionUpdate
    ) throws -> RadrootsTodayRefreshReceipt {
        if let refreshFailure {
            throw refreshFailure
        }
        return RadrootsTodayRefreshReceipt(
          update: update,
          sourceEvents: 0,
          visibleCards: 0,
          profiles: 0,
          threadEntries: 0,
          contentGeneration: 1,
          changed: false
        )
    }

    func subscribe(
      bufferCapacity _: Int,
      receive _: @escaping @Sendable (RadrootsRuntimeChange) async -> Void
    ) -> any RadrootsRuntimeSubscriptionToken {
        TodaySubscriptionToken()
    }

    func shutdown() -> RadrootsRuntimeShutdownReceipt {
        let alreadyClosed = closed
        closed = true
        return RadrootsRuntimeShutdownReceipt(state: "closed", alreadyClosed: alreadyClosed)
    }
}

private actor TodaySubscriptionToken: RadrootsRuntimeSubscriptionToken {
    func cancel() {}
}
