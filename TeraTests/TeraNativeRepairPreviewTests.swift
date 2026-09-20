import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraNativeRepairPreviewTests: XCTestCase {
  func testLaterPageCannotHideEarlierRepairAndDurableResolutionClearsIt() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let issue = Self.issue(1)
    let pages = NativeRepairPages([
      .init(visited: 64, remaining: 100, needsAttention: true, issues: [issue]),
      .init(visited: 1, remaining: 0, needsAttention: false),
    ])
    let store = TeraNativeRepairStore(client: client, media: pages)
    store.configure(author: "first")
    await store.reconcile()
    await store.reconcile()
    XCTAssertEqual(store.issues, [issue])
    XCTAssertNotNil(store.message)
    await backend.setNativeRepairValue(.init(key: issue.key, reason: .resolved, revision: 2, firstObservedUnixMS: 1, updatedAtUnixMS: 2))
    await store.reconcile()
    XCTAssertTrue(store.issues.isEmpty)
    XCTAssertNil(store.message)
    store.configure(author: "second")
    XCTAssertNil(store.progress)
    store.stop()
    _ = try await client.stop()
  }

  func testRepairPreviewRemainsBoundedAcrossFullPages() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let pages = NativeRepairPages([
      .init(visited: 64, remaining: 64, needsAttention: true, issues: (1 ... 64).map(Self.issue)),
      .init(visited: 64, remaining: 0, needsAttention: true, issues: (65 ... 128).map(Self.issue)),
    ])
    let store = TeraNativeRepairStore(client: client, media: pages)
    await store.reconcile()
    await store.reconcile()
    XCTAssertEqual(store.issues.count, 64)
    XCTAssertEqual(Set(store.issues.map(\.key)), Set((1 ... 64).map { Self.issue($0).key }))
    store.stop()
    _ = try await client.stop()
  }

  private static func issue(_ index: Int) -> TeraNativeRecoveryIssue {
    .init(key: String(format: "%064x", index), reason: .missingParent, status: nil)
  }
}

extension TeraScopeBackend {
  func nativeRecoveryStatus(key: String) -> TeraNativeRecoveryStatus? {
    nativeRepairValues[key]
  }

  func setNativeRepairValue(_ value: TeraNativeRecoveryStatus) {
    nativeRepairValues[value.key] = value
  }
}

private actor NativeRepairPages: TeraAddMediaHandling {
  var pages: [TeraNativeRecoveryProgress]
  init(_ pages: [TeraNativeRecoveryProgress]) {
    self.pages = pages
  }

  func support() -> TeraAddMediaSupport {
    .unavailable
  }

  func recoverNativeUploads(client _: TeraRuntimeClient) -> TeraNativeRecoveryProgress {
    pages.isEmpty ? .init(visited: 0, remaining: 0, needsAttention: false) : pages.removeFirst()
  }

  func importImages(limit _: Int) throws -> [TeraPreparedMedia] {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func captureImage() throws -> TeraPreparedMedia {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func open(_: [TeraPreparedMedia]) throws -> TeraOpenedMedia {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}
