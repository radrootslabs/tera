import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraCapacityRecoveryTests: XCTestCase {
  func testTypedNativeCapacityAndReceiptQuotaRemainDistinctAndNeverPromiseSavedEditing() {
    let capacity: [Error] = [RadrootsAppleFileError.spaceInsufficient, RadrootsCaptureIntakeError.spaceInsufficient, RadrootsBackgroundTransferError.spaceInsufficient]
    for error in capacity {
      XCTAssertEqual(TeraUserMessages.key(for: error, fallback: .fileOperationFailed), .storageFull)
      XCTAssertEqual(TeraNativeRecoveryClassification.pause(error), .quota)
    }
    let quota = RadrootsBackgroundTransferError.receiptCapacityExceeded
    XCTAssertEqual(TeraUserMessages.key(for: quota, fallback: .fileOperationFailed), .receiptCapacityExceeded)
    XCTAssertEqual(TeraNativeRecoveryClassification.pause(quota), .receiptQuota)
    XCTAssertTrue(TeraUserMessages.text(.receiptCapacityExceeded).contains("does not resolve"))
    XCTAssertTrue(TeraUserMessages.text(.storageFull).contains("check the original work before retrying"))
    XCTAssertTrue(TeraUserMessages.text(.storageFull).contains("Saving may be unavailable"))
    for pause in [TeraNativeRecoveryPause.quota, .receiptQuota, .storageUnavailable] {
      XCTAssertFalse(pause.message.contains("Saved editing is still available"))
      XCTAssertTrue(pause.message.contains("new saves may be unavailable"))
    }
  }

  @MainActor
  func testCacheCleanupAdmissionSurvivesStopAndLateResultsCannotOverwritePresentation() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraSettingsStore(runtimeClient: client)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let pause = await backend.pause(.cacheCleanup)
    let owner = Task { await store.cleanupMediaCache(context: context) }
    await pause.entered.wait()
    store.stop()
    await store.cleanupMediaCache(context: context)
    let firstCount = await backend.counts[.cacheCleanup]
    XCTAssertEqual(firstCount, 1)
    await pause.resume.open()
    await owner.value
    XCTAssertNil(store.message)
    await store.cleanupMediaCache(context: context)
    let secondCount = await backend.counts[.cacheCleanup]
    XCTAssertEqual(secondCount, 2)
    XCTAssertTrue(store.message?.contains("1 shared or unverified") == true)
    XCTAssertFalse(store.message?.contains("bytes freed") == true)
    _ = try await client.stop()
  }

  @MainActor
  func testFailedCacheCleanupDoesNotReportSuccessOrDiscardEditing() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraSettingsStore(runtimeClient: client)
    store.profileName = "Unsaved profile"
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let failure = TeraRuntimeFailure.local(operation: "test", code: "storage_space_insufficient", safeMessage: "space")
    let pause = await backend.pause(.cacheCleanup, failure: failure)
    let owner = Task { await store.cleanupMediaCache(context: context) }
    await pause.entered.wait()
    await pause.resume.open()
    await owner.value
    XCTAssertEqual(store.profileName, "Unsaved profile")
    XCTAssertEqual(store.failureCode, "storage_space_insufficient")
    XCTAssertEqual(store.message, TeraUserMessages.text(.storageFull))
    _ = try await client.stop()
  }
}
