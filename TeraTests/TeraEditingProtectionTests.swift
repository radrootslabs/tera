import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraEditingProtectionTests: XCTestCase {
  func testLateDialogDismissalOrDiscardCannotActOnANewerFailedChoice() async throws {
    let protection = TeraEditingProtection()
    var applied = 0
    _ = await protection.replace(kind: .editing, save: { false }, apply: { applied += 1; return true })
    let old = try XCTUnwrap(protection.choiceToken)
    protection.cancel(token: old)
    _ = await protection.replace(kind: .editing, save: { false }, apply: { applied += 1; return true })
    let current = try XCTUnwrap(protection.choiceToken)
    XCTAssertNotEqual(old, current)
    protection.cancel(token: old)
    protection.discard(token: old)
    XCTAssertTrue(protection.failed)
    XCTAssertEqual(protection.choiceToken, current)
    XCTAssertEqual(applied, 0)
    protection.discard(token: current)
    await TeraScopeFixtures.eventually { !protection.isWorking }
    XCTAssertEqual(applied, 1)
  }

  func testOnePendingReplacementAwaitsSaveAndPublishesOnlyAppliedReopen() async {
    let protection = TeraEditingProtection()
    let pause = ResourceTestPause()
    var saves = 0
    var applied = 0
    let first = Task {
      await protection.replace(kind: .reopen, save: {
        saves += 1
        await pause.wait()
        return true
      }, apply: { applied += 1; return true })
    }
    await pause.entered.wait()
    for _ in 0 ..< 1000 {
      XCTAssertNil(protection.schedule(kind: .editing, save: { XCTFail("Duplicate save"); return true }, apply: { false }))
      protection.retry()
      protection.discard()
    }
    XCTAssertEqual(saves, 1)
    XCTAssertEqual(applied, 0)
    XCTAssertNil(protection.reopened)
    await pause.resume.open()
    let result = await first.value
    XCTAssertTrue(result)
    XCTAssertEqual(applied, 1)
    XCTAssertNotNil(protection.reopened)
    XCTAssertFalse(protection.failed)
    XCTAssertFalse(protection.isWorking)
  }

  func testFailedSaveRetainsChoiceAndRetryAppliesOnce() async {
    let protection = TeraEditingProtection()
    var saves = 0
    var applied = 0
    let result = await protection.replace(kind: .reopen, save: { saves += 1; return saves == 2 }, apply: { applied += 1; return true })
    XCTAssertFalse(result)
    XCTAssertTrue(protection.failed)
    XCTAssertNil(protection.reopened)
    XCTAssertEqual(applied, 0)
    protection.retry()
    await TeraScopeFixtures.eventually { !protection.isWorking }
    XCTAssertEqual(saves, 2)
    XCTAssertEqual(applied, 1)
    XCTAssertNotNil(protection.reopened)
    protection.retry()
    protection.discard()
    XCTAssertEqual(applied, 1)
  }

  func testDiscardIsExplicitAndCancellingTheChoiceKeepsEditing() async {
    let protection = TeraEditingProtection()
    var applied = 0
    var saves = 0
    let save = { saves += 1; return false }
    let apply = { applied += 1; return true }
    _ = await protection.replace(kind: .editing, save: save, apply: apply)
    protection.cancel()
    protection.discard()
    XCTAssertEqual(applied, 0)
    XCTAssertFalse(protection.failed)
    _ = await protection.replace(kind: .editing, save: save, apply: apply)
    XCTAssertTrue(protection.failed)
    protection.discard()
    await TeraScopeFixtures.eventually { !protection.isWorking }
    XCTAssertEqual(applied, 1)
    XCTAssertEqual(saves, 2)
    XCTAssertNil(protection.reopened)
  }

  func testEditingChangedWhileSavingRetainsWorkerUntilLateCompletionWithoutReplacing() async {
    let protection = TeraEditingProtection()
    let pause = ResourceTestPause()
    var applied = false
    let task = Task {
      await protection.replace(kind: .reopen, save: { await pause.wait(); return true }, apply: { applied = true; return true })
    }
    await pause.entered.wait()
    protection.editingChanged()
    XCTAssertNil(protection.schedule(kind: .editing, save: { true }, apply: { true }))
    await pause.resume.open()
    let result = await task.value
    XCTAssertFalse(result)
    XCTAssertFalse(applied)
    XCTAssertNil(protection.reopened)
    let next = await protection.replace(kind: .editing, save: { true }, apply: { true })
    XCTAssertTrue(next)
  }

  func testCancelledWaiterCannotReplaceAndFailedLoadCannotDismissSavedWork() async {
    let protection = TeraEditingProtection()
    let pause = ResourceTestPause()
    let task = Task {
      await protection.replace(kind: .reopen, save: { await pause.wait(); return true }, apply: { XCTFail("Cancelled replacement"); return true })
    }
    await pause.entered.wait()
    task.cancel()
    await TeraScopeFixtures.eventually { !protection.isWorking }
    await pause.resume.open()
    let result = await task.value
    XCTAssertFalse(result)
    let refused = await protection.replace(kind: .reopen, save: { true }, apply: { false })
    XCTAssertFalse(refused)
    XCTAssertNil(protection.reopened)
    XCTAssertFalse(protection.failed)
  }
}
