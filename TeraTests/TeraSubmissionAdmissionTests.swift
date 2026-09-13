import Foundation
@testable import TeraApp
import XCTest

final class TeraSubmissionAdmissionTests: XCTestCase {
  func testTimeoutOrCancellationReturnsPromptlyButKeepsAdmissionUntilActualCallback() async throws {
    for cancel in [false, true] {
      let backend = AddBackend()
      let client = TeraRuntimeClient(factory: { _ in
        await TeraRuntimeBackendStart(backend: backend, snapshot: backend.snapshot())
      }, deadlines: TeraRuntimeDeadlinePolicy(operationNanoseconds: 100_000_000))
      _ = try await client.start(configuration: TeraAddStoreTests.configuration())
      let scope = TeraComposerScope(authorPublicKey: String(repeating: "ab", count: 32), localNetworkID: "nearby")
      var form = TeraComposerForm(commandType: .createUpdate)
      form.content = "original"
      let saved = try await client.saveComposer(request: TeraComposerSaveRequest(
        scope: scope, id: client.reserveComposerID(), expectedRevision: nil, editSequence: 1, form: form
      ))
      let request = try await TeraSubmissionRequest(commandID: client.reserveSubmissionID(), scope: scope,
                                                    composerID: saved.draft.id, expectedRevision: saved.draft.revision)
      let pause = ResourceTestPause()
      await backend.submissionBackend.pausePrepare(pause)
      let first = Task { try await client.prepareSubmission(request: request, media: []) }
      let entered = expectation(description: "Reached the original submission call")
      let observer = Task { await pause.entered.wait(); entered.fulfill() }
      await fulfillment(of: [entered], timeout: 3)
      await pause.entered.open()
      await observer.value
      if cancel {
        first.cancel()
      }
      await assertBoundedReturn(first, cancelled: cancel)
      await assertHeldAdmission(client, request: request)
      let calls = await backend.submissionBackend.prepareCount
      XCTAssertEqual(calls, 1)
      await pause.resume.open()
      var recovered: TeraSubmissionStatus?
      let deadline = ContinuousClock.now.advanced(by: .seconds(3))
      while recovered == nil, ContinuousClock.now < deadline {
        recovered = try? await client.recoverSubmission(request: request)
        if recovered == nil {
          try await Task.sleep(for: .milliseconds(1))
        }
      }
      let original = try XCTUnwrap(recovered)
      let replay = try await client.prepareSubmission(request: request, media: [])
      XCTAssertEqual(replay, original)
      XCTAssertEqual(replay.request, request)
      XCTAssertEqual(replay.captured, saved.draft)
      let operations = await backend.submissionBackend.operationCount
      XCTAssertEqual(operations, 1)
      _ = try await client.stop()
    }
  }

  private func assertHeldAdmission(_ client: TeraRuntimeClient, request: TeraSubmissionRequest) async {
      for _ in 0 ..< 100 {
        do {
          _ = try await client.prepareSubmission(request: request, media: [])
          XCTFail("A late original callback must retain admission")
        } catch { XCTAssertEqual(TeraAddPresentation.failure(for: error)?.code, "operation_in_progress") }
      }
      do {
        _ = try await client.recoverSubmission(request: request)
        XCTFail("Recovery cannot infer absence while the original call is unfinished")
      } catch { XCTAssertEqual(TeraAddPresentation.failure(for: error)?.code, "operation_in_progress") }
  }

  private func assertBoundedReturn(_ first: Task<TeraSubmissionStatus, Error>, cancelled cancel: Bool) async {
      let returned = expectation(description: "Native waiter keeps its original deadline")
      let waiter = Task {
        do {
          _ = try await first.value
          XCTFail("Expected bounded wait to end")
        } catch {
          XCTAssertEqual(TeraAddPresentation.failure(for: error)?.code,
                         cancel ? "ios.runtime.cancelled" : "ios.runtime.deadline_exceeded")
        }
        returned.fulfill()
      }
      await fulfillment(of: [returned], timeout: 2)
      await waiter.value
  }
}
