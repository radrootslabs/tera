@testable import TeraApp
import XCTest

final class TeraRuntimeResourceTaskTests: XCTestCase {
  func testCompletedResourceIsCleanedOnceWhenAbandonedBeforeAdoption() async {
    let cleaned = expectation(description: "Completed but unadopted resource is closed")
    cleaned.assertForOverFulfill = true
    let task = TeraRuntimeResourceTask<Int>(
      deadlineNanoseconds: .max, cleanupDeadlineNanoseconds: .max,
      operation: { .success(42) }, cleanup: { value in
        XCTAssertEqual(value, 42)
        cleaned.fulfill()
      }
    )
    guard case .completed(.success(42)) = await task.value() else { return XCTFail("Creation must complete") }
    task.cancel()
    task.cancel()
    XCTAssertFalse(task.adopt())
    await fulfillment(of: [cleaned], timeout: 2)
  }

  func testCancellationBeforeCreationCleansLateSuccessOnce() async {
    let pause = ResourceTestPause()
    let cleaned = expectation(description: "Late resource is closed")
    cleaned.assertForOverFulfill = true
    let task = TeraRuntimeResourceTask<Int>(
      deadlineNanoseconds: .max, cleanupDeadlineNanoseconds: .max,
      operation: { await pause.wait(); return .success(42) },
      cleanup: { _ in cleaned.fulfill() }
    )
    await pause.entered.wait()
    task.cancel()
    task.cancel()
    guard case .cancelled = await task.value() else { return XCTFail("Caller must stop waiting") }
    await pause.resume.open()
    await fulfillment(of: [cleaned], timeout: 2)
    XCTAssertFalse(task.adopt())
  }

  func testTimeoutDuringCreationCleansLateSuccessOnce() async {
    let pause = ResourceTestPause()
    let cleaned = expectation(description: "Timed out resource is closed")
    cleaned.assertForOverFulfill = true
    let task = TeraRuntimeResourceTask<Int>(
      deadlineNanoseconds: 1_000_000, cleanupDeadlineNanoseconds: .max,
      operation: { await pause.wait(); return .success(42) },
      cleanup: { _ in cleaned.fulfill() }
    )
    await pause.entered.wait()
    guard case .timedOut = await task.value() else { return XCTFail("Deadline must end the wait") }
    task.cancel()
    await pause.resume.open()
    await fulfillment(of: [cleaned], timeout: 2)
    XCTAssertFalse(task.adopt())
  }

  func testAdoptedResourceCannotBeCleanedByAnOldWaiter() async {
    let task = TeraRuntimeResourceTask<Int>(
      deadlineNanoseconds: .max, cleanupDeadlineNanoseconds: .max,
      operation: { .success(42) }, cleanup: { _ in XCTFail("The active owner holds this resource") }
    )
    guard case .completed(.success(42)) = await task.value() else { return XCTFail("Creation must complete") }
    XCTAssertTrue(task.adopt())
    XCTAssertFalse(task.adopt())
    task.cancel()
    task.cancel()
    guard case .completed(.success(42)) = await task.value() else { return XCTFail("Keep the useful result") }
  }

  func testCreationFailureHasNoResourceToClean() async {
    let failure = TeraRuntimeFailure.local(operation: "test", code: "test.failure", safeMessage: "Test failure")
    let task = TeraRuntimeResourceTask<Int>(
      deadlineNanoseconds: .max, cleanupDeadlineNanoseconds: .max,
      operation: { .failure(failure) }, cleanup: { _ in XCTFail("No resource was created") }
    )
    guard case let .completed(.failure(observed)) = await task.value() else { return XCTFail("Failure must survive") }
    XCTAssertEqual(observed, failure)
    task.cancel()
    XCTAssertFalse(task.adopt())
  }
}
