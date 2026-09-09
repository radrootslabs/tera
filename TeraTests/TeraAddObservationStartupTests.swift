@testable import TeraApp
import XCTest

@MainActor
final class TeraAddObservationStartupTests: XCTestCase {
  func testPendingObserverCannotDiscardUsableStartupInventory() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let media = ObservationStartupMedia()
    let subscription = await backend.pause(.subscribe)
    let initialDrafts = await backend.pause(.drafts)
    let store = TeraAddStore(runtimeClient: client, media: media)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    let startup = Task { await store.start() }
    await initialDrafts.entered.wait()
    await media.supportPause.entered.wait()
    let observerDrafts = await backend.pause(.drafts)
    await initialDrafts.resume.open()
    await subscription.resume.open()
    await observerDrafts.entered.wait()
    await media.supportPause.resume.open()
    await startup.value
    XCTAssertEqual(store.drafts.first?.form?.content, "old")
    XCTAssertEqual(store.mediaSupport, .init(library: true, camera: true))
    XCTAssertEqual(store.state, .ready)
    let inventories = await media.inventories
    XCTAssertEqual(inventories.map { $0.first?.revision }, [1])
    await observerDrafts.resume.open()
    store.stop()
    _ = try await client.stop()
  }

  func testStartupReconcilesTheAppliedObserverInventoryAndKeepsMediaSupport() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let media = ObservationStartupMedia()
    let subscription = await backend.pause(.subscribe)
    let initialDrafts = await backend.pause(.drafts)
    let store = TeraAddStore(runtimeClient: client, media: media)
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    let startup = Task { await store.start() }
    await initialDrafts.entered.wait()
    await media.supportPause.entered.wait()
    await initialDrafts.resume.open()
    await backend.setDrafts([TeraScopeFixtures.draft("new", revision: 2)])
    await backend.configure(TeraScopeFixtures.snapshot(evidence: TeraScopeFixtures.evidence(observedAt: 2)))
    await subscription.resume.open()
    await TeraScopeFixtures.eventually {
      store.drafts.first?.revision == 2 && store.blossomEvidence?.observedAtUnixMilliseconds == 2
    }
    await media.supportPause.resume.open()
    await startup.value
    XCTAssertEqual(store.drafts.first?.revision, 2)
    XCTAssertEqual(store.mediaSupport, .init(library: true, camera: true))
    let inventories = await media.inventories
    XCTAssertEqual(inventories.map { $0.first?.revision }, [2])
    store.stop()
    _ = try await client.stop()
  }
}

private actor ObservationStartupMedia: TeraAddMediaHandling {
  let supportPause = ResourceTestPause()
  private(set) var inventories: [[TeraDraftStatus]] = []

  func support() async -> TeraAddMediaSupport {
    await supportPause.wait()
    return .init(library: true, camera: true)
  }

  func reconcileBackgroundUploads(drafts: [TeraDraftStatus]) {
    inventories.append(drafts)
  }

  func importImages(limit _: Int) throws -> [TeraPreparedMedia] {
    throw TeraScopeFixtures.failure()
  }

  func captureImage() throws -> TeraPreparedMedia {
    throw TeraScopeFixtures.failure()
  }

  func open(_: [TeraPreparedMedia]) throws -> TeraOpenedMedia {
    throw TeraScopeFixtures.failure()
  }
}
