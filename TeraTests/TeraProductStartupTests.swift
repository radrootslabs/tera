@testable import TeraApp
import XCTest

@MainActor
final class TeraProductStartupTests: XCTestCase {
  func testLocalAddIsEditableWhileTodayRefreshHasNotReturned() async throws {
    let fixture = try await StartupFixture()
    let refresh = await fixture.backend.pause(.refresh)
    fixture.stores.start()
    await refresh.entered.wait()
    await TeraScopeFixtures.eventually { fixture.stores.add.isProductReady }
    XCTAssertEqual(Set(fixture.stores.add.schemas.map(\.commandType)), Set(TeraAddCommandType.allCases))
    XCTAssertEqual(fixture.stores.add.drafts.first?.form?.content, "old")
    XCTAssertEqual(fixture.stores.today.presentation.refresh, .refreshing)
    fixture.stores.add.updateForm(\.content, "Compose while refresh is held")
    XCTAssertTrue(fixture.stores.add.canSave)
    XCTAssertEqual(fixture.stores.add.form.content, "Compose while refresh is held")
    fixture.stores.suspend()
    await refresh.resume.open()
    try await fixture.close()
  }

  func testSuspendCancelsBothStartupReadsBeforeTheirBackendsReturn() async throws {
    let fixture = try await StartupFixture()
    let page = await fixture.backend.pause(.page)
    let drafts = await fixture.backend.pause(.drafts)
    let task = Task { await fixture.stores.resume() }
    await page.entered.wait()
    await drafts.entered.wait()
    fixture.stores.suspend()
    await task.value
    XCTAssertTrue(fixture.stores.today.cards.isEmpty)
    XCTAssertTrue(fixture.stores.add.schemas.isEmpty)
    XCTAssertTrue(fixture.stores.add.drafts.isEmpty)
    XCTAssertFalse(fixture.stores.add.isProductReady)
    XCTAssertEqual(fixture.stores.today.observationState, .stopped)
    XCTAssertEqual(fixture.stores.add.observationState, .stopped)
    await page.resume.open()
    await drafts.resume.open()
    try await fixture.close()
    XCTAssertTrue(fixture.stores.add.schemas.isEmpty)
    XCTAssertTrue(fixture.stores.add.drafts.isEmpty)
    XCTAssertTrue(fixture.stores.today.cards.isEmpty)
  }

  func testCanceledResumeStopsBothObserversAndCanStartAgain() async throws {
    let fixture = try await StartupFixture()
    let refresh = await fixture.backend.pause(.refresh)
    let drafts = await fixture.backend.pause(.drafts)
    let task = Task { await fixture.stores.resume() }
    await refresh.entered.wait()
    await drafts.entered.wait()
    task.cancel()
    await task.value
    XCTAssertEqual(fixture.stores.today.observationState, .stopped)
    XCTAssertEqual(fixture.stores.add.observationState, .stopped)
    XCTAssertFalse(fixture.stores.add.isProductReady)
    await refresh.resume.open()
    await drafts.resume.open()
    await fixture.releaseObservers()
    await fixture.stores.resume()
    XCTAssertTrue(fixture.stores.add.isProductReady)
    XCTAssertEqual(fixture.stores.add.schemas.count, 5)
    try await fixture.close()
  }

  func testConcurrentStartsAndResumesKeepOneObserverPerStore() async throws {
    let fixture = try await StartupFixture()
    let refresh = await fixture.backend.pause(.refresh)
    fixture.stores.start()
    fixture.stores.start()
    let first = Task { await fixture.stores.resume() }
    let second = Task { await fixture.stores.resume() }
    await refresh.entered.wait()
    await TeraScopeFixtures.eventually { fixture.stores.add.isProductReady }
    for observer in fixture.observers {
      await observer.entered.wait()
    }
    let counts = await fixture.backend.counts
    XCTAssertEqual(counts[.subscribe], 2)
    XCTAssertEqual(counts[.refresh], 1)
    XCTAssertEqual(counts[.drafts], 1)
    await fixture.releaseObservers()
    await refresh.resume.open()
    await first.value
    await second.value
    let final = await fixture.backend.counts
    XCTAssertEqual(final[.subscribe], 2)
    XCTAssertEqual(final[.refresh], 1)
    try await fixture.close()
  }

  func testReconfigurationRejectsOldStartupAndKeepsTheReplacementOwned() async throws {
    for changed in [false, true] {
      try await assertReplacement(changed: changed)
    }
  }

  func testSuspendAndResumePreserveTheUnsentEditingForm() async throws {
    let fixture = try await StartupFixture()
    await fixture.releaseObservers()
    await fixture.stores.resume()
    fixture.stores.add.updateForm(\.content, "Unsent local editing")
    fixture.stores.suspend()
    await fixture.stores.resume()
    XCTAssertTrue(fixture.stores.add.isProductReady)
    XCTAssertEqual(fixture.stores.add.form.content, "Unsent local editing")
    try await fixture.close()
  }

  private func assertReplacement(changed: Bool) async throws {
    let fixture = try await StartupFixture()
    let oldRefresh = await fixture.backend.pause(.refresh)
    let oldDrafts = await fixture.backend.pause(.drafts)
    let old = Task { await fixture.stores.resume() }
    await oldRefresh.entered.wait()
    await oldDrafts.entered.wait()
    let snapshot = TeraScopeFixtures.snapshot(relay: changed ? "second" : "first")
    await fixture.backend.configure(snapshot)
    await fixture.backend.setDrafts([TeraScopeFixtures.draft("replacement", revision: 2)])
    fixture.stores.configure(snapshot: snapshot)
    let newRefresh = await fixture.backend.pause(.refresh)
    let current = Task { await fixture.stores.resume() }
    await newRefresh.entered.wait()
    await old.value
    await TeraScopeFixtures.eventually { fixture.stores.add.isProductReady }
    XCTAssertEqual(fixture.stores.add.drafts.first?.form?.content, "replacement")
    XCTAssertEqual(fixture.stores.today.selectedContext?.relayURLs, snapshot.relay?.relays.map(\.url))
    fixture.stores.suspend()
    await current.value
    XCTAssertEqual(fixture.stores.today.observationState, .stopped)
    await oldRefresh.resume.open()
    await oldDrafts.resume.open()
    await newRefresh.resume.open()
    try await fixture.close()
    XCTAssertEqual(fixture.stores.add.drafts.first?.form?.content, "replacement")
  }
}

@MainActor
private struct StartupFixture {
  let backend: TeraScopeBackend
  let client: TeraRuntimeClient
  let stores: TeraProductStores
  let observers: [ResourceTestPause]

  init() async throws {
    backend = try TeraScopeBackend()
    client = try await TeraScopeFixtures.client(backend)
    stores = TeraProductStores(runtimeClient: client)
    stores.configure(snapshot: TeraScopeFixtures.snapshot())
    observers = await [backend.pause(.subscribe), backend.pause(.subscribe)]
  }

  func releaseObservers() async {
    for observer in observers {
      await observer.resume.open()
    }
  }

  func close() async throws {
    stores.stop()
    await releaseObservers()
    _ = try await client.stop()
  }
}
