import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraTodayCalendarScopeTests: XCTestCase {
  private var zone = TimeZone.gmt

  func testTravelKeepsLoadedContextUntilExplicitRefresh() async throws {
    zone = try XCTUnwrap(TimeZone(identifier: "America/Vancouver"))
    let west = TeraScopeFixtures.viewerCalendar(asOf: 1_788_569_400, timeZone: zone)
    let backend = try TeraScopeBackend()
    await backend.setPage(page("one", calendar: west, next: "next"))
    await backend.setPage(page("two", calendar: west), cursor: "next")
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(
      runtimeClient: client, clock: .fixed(unixSeconds: west.asOfUnixSeconds),
      viewerTimeZone: { self.zone }
    )
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.reload(refreshProjection: false)
    XCTAssertEqual(store.viewerCalendar, west)
    zone = try XCTUnwrap(TimeZone(identifier: "Pacific/Kiritimati"))
    await store.loadNextPage()
    XCTAssertEqual(store.cards.map(\.id), ["one", "two"])
    XCTAssertEqual(store.viewerCalendar, west)
    let requests = await backend.pageRequests
    XCTAssertEqual(requests.first?.viewerTimeZone, west.timeZoneID)
    XCTAssertNil(requests.last?.viewerTimeZone)
    XCTAssertNil(requests.last?.asOfUnixSeconds)
    let reconciled = try await TeraTodayReconciler.read(
      client: client, context: XCTUnwrap(store.selectedContext), calendar: west, cards: store.cards
    )
    XCTAssertEqual(reconciled.calendar, west)
    let east = TeraScopeFixtures.viewerCalendar(asOf: west.asOfUnixSeconds, timeZone: zone)
    await backend.setPage(page("fresh", calendar: east))
    await store.reload(refreshProjection: false)
    XCTAssertEqual(store.viewerCalendar, east)
    XCTAssertEqual(store.cards.map(\.id), ["fresh"])
    let refreshed = await backend.pageRequests
    XCTAssertEqual(refreshed.last?.viewerTimeZone, east.timeZoneID)
    _ = try await client.stop()
  }

  func testMismatchedCalendarCannotAppendEvenWithSameInstantAndGeneration() async throws {
    let west = TeraScopeFixtures.viewerCalendar(asOf: 1)
    let east = try TeraScopeFixtures.viewerCalendar(asOf: 1, timeZone: XCTUnwrap(TimeZone(identifier: "Pacific/Kiritimati")))
    let backend = try TeraScopeBackend()
    await backend.setPage(page("one", calendar: west, next: "next"))
    await backend.setPage(page("wrong", calendar: east), cursor: "next")
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraTodayStore(runtimeClient: client, clock: .fixed(unixSeconds: 1))
    store.configure(snapshot: TeraScopeFixtures.snapshot())
    await store.reload(refreshProjection: false)
    await store.loadNextPage()
    XCTAssertEqual(store.cards.map(\.id), ["one"])
    XCTAssertEqual(store.viewerCalendar, west)
    XCTAssertFalse(store.canLoadNextPage)
    XCTAssertTrue(store.presentation.readFailure?.requiresRefresh == true)
    _ = try await client.stop()
  }

  private func page(_ id: String, calendar: TeraViewerCalendarContext, next: String? = nil) -> TeraTodayPage {
    TeraTodayPage(
      asOfUnixSeconds: calendar.asOfUnixSeconds, items: [TeraScopeFixtures.card(id)],
      nextCursor: next, projectionGeneration: 1, calendar: calendar
    )
  }
}
