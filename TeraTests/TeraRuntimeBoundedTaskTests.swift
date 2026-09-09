import Foundation
@testable import TeraApp
import XCTest

final class TeraRuntimeBoundedTaskTests: XCTestCase {
  private typealias Bounded = TeraRuntimeBoundedTask<Int>

  func testCompletionTimeoutAndCancellationBeforeInstallation() async {
    for terminal in Terminal.allCases {
      await assertInstallation(terminal: terminal, terminalFirst: true)
    }
  }

  func testCompletionTimeoutAndCancellationAfterInstallation() async {
    for terminal in Terminal.allCases {
      await assertInstallation(terminal: terminal, terminalFirst: false)
    }
  }

  func testCompletedFailureDoesNotCancelLateInstalledOperation() async {
    let state = Bounded.State()
    let gate = BoundedTestGate()
    let operation = Task { await gate.wait() }
    let timer = Task { await gate.wait() }
    let failure = TeraRuntimeFailure.local(operation: "test", code: "test.failure", safeMessage: "Test failure")
    XCTAssertTrue(state.resolve(.completed(.failure(failure))))
    state.install(operationTask: operation, timeoutTask: timer)
    XCTAssertFalse(operation.isCancelled)
    XCTAssertTrue(timer.isCancelled)
    if case let .completed(.failure(observed)) = await state.value(cancelsOperationWhenWaiterCancelled: true) {
      XCTAssertEqual(observed, failure)
    } else {
      XCTFail("The first completed result must remain owned by its caller")
    }
    await gate.open()
    await operation.value
    await timer.value
  }

  func testAbandonedSuccessfulOperationStillRunsItsCleanupExactlyOnce() async {
    let started = BoundedTestGate()
    let release = BoundedTestGate()
    let abandoned = expectation(description: "Abandoned result cleanup")
    abandoned.assertForOverFulfill = true
    let task = Bounded(deadlineNanoseconds: .max) {
      await started.open()
      await release.wait()
      return .success(42)
    } onAbandonedResult: { result in
      XCTAssertEqual(try? result.get(), 42)
      abandoned.fulfill()
    }
    await started.wait()
    task.cancel()
    task.cancel()
    if case .cancelled = await task.value() {} else {
      XCTFail("Cancellation must resolve the caller before the operation returns")
    }
    await release.open()
    await fulfillment(of: [abandoned], timeout: 2)
  }

  func testSuccessfulWrapperKeepsItsResultAfterLaterCancellation() async {
    let task = Bounded(deadlineNanoseconds: .max, operation: { .success(42) }) { _ in
      XCTFail("A result owned by its caller must not be abandoned")
    }
    await assertOutcome(task.value(), terminal: .completion)
    task.cancel()
    await assertOutcome(task.value(), terminal: .completion)
  }

  private func assertInstallation(terminal: Terminal, terminalFirst: Bool) async {
    let state = Bounded.State()
    let gate = BoundedTestGate()
    let operation = Task { await gate.wait() }
    let timer = Task { await gate.wait() }
    if terminalFirst {
      terminal.resolve(state)
    }
    state.install(operationTask: operation, timeoutTask: timer)
    if !terminalFirst {
      terminal.resolve(state)
    }
    XCTAssertEqual(operation.isCancelled, terminal != .completion, "\(terminal), before=\(terminalFirst)")
    XCTAssertTrue(timer.isCancelled)
    await assertOutcome(state.value(cancelsOperationWhenWaiterCancelled: true), terminal: terminal)
    XCTAssertFalse(state.resolve(.completed(.success(99))))
    state.cancel()
    state.expire()
    await assertOutcome(state.value(cancelsOperationWhenWaiterCancelled: true), terminal: terminal)
    await gate.open()
    await operation.value
    await timer.value
    state.finishOperation()
  }

  private func assertOutcome(_ outcome: Bounded.Outcome, terminal: Terminal) {
    switch (terminal, outcome) {
    case let (.completion, .completed(.success(value))): XCTAssertEqual(value, 42)
    case (.timeout, .timedOut), (.cancellation, .cancelled): break
    default: XCTFail("The first terminal outcome must resolve each wait exactly once")
    }
  }

  private enum Terminal: CaseIterable {
    case completion, timeout, cancellation

    func resolve(_ state: Bounded.State) {
      switch self {
      case .completion: XCTAssertTrue(state.resolve(.completed(.success(42))))
      case .timeout: state.expire()
      case .cancellation: state.cancel()
      }
    }
  }
}

private actor BoundedTestGate {
  private var opened = false
  private var waiters: [CheckedContinuation<Void, Never>] = []

  func wait() async {
    guard !opened else { return }
    await withCheckedContinuation { waiters.append($0) }
  }

  func open() {
    guard !opened else { return }
    opened = true
    let pending = waiters
    waiters.removeAll()
    for waiter in pending {
      waiter.resume()
    }
  }
}
