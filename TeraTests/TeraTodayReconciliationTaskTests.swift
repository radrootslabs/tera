@testable import TeraApp
import XCTest

@MainActor
final class TeraTodayReconciliationTaskTests: XCTestCase {
  func testCancelledPageWaiterCannotCancelMandatoryVisibilityRefresh() async {
    let owner = TeraTodayReconciliationTask()
    let pause = ResourceTestPause()
    var refreshWasCancelled: Bool?
    let refresh = Task {
      await owner.run {
        await pause.wait()
        refreshWasCancelled = Task.isCancelled
      }
    }
    await pause.entered.wait()
    var waiting = false
    let page = Task { waiting = true; await owner.wait() }
    await TeraScopeFixtures.eventually { waiting }
    page.cancel()
    await pause.resume.open()
    await refresh.value
    await page.value
    XCTAssertEqual(refreshWasCancelled, false)
  }

  func testPagingWaitsUntilReconciliationHasInstalledItsResult() async {
    let owner = TeraTodayReconciliationTask()
    let pause = ResourceTestPause()
    var visible = "old"
    let refresh = Task { await owner.run { await pause.wait(); visible = "current" } }
    await pause.entered.wait()
    var pageValue: String?
    let page = Task { await owner.wait(); pageValue = visible }
    XCTAssertNil(pageValue)
    await pause.resume.open()
    await refresh.value
    await page.value
    XCTAssertEqual(pageValue, "current")
  }

  func testOldCancellationCannotClearReplacementReconciliation() async {
    let owner = TeraTodayReconciliationTask()
    let old = ResourceTestPause()
    let current = ResourceTestPause()
    let oldTask = Task { await owner.run { await old.wait() } }
    await old.entered.wait()
    let newTask = Task { await owner.run { await current.wait() } }
    await current.entered.wait()
    await old.resume.open()
    await oldTask.value
    var completed = false
    let waiting = Task { await owner.wait(); completed = true }
    XCTAssertFalse(completed)
    await current.resume.open()
    await newTask.value
    await waiting.value
    XCTAssertTrue(completed)
    owner.cancel()
    await owner.wait()
  }
}
