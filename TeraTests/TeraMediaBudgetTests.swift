import Foundation
@testable import TeraApp
import UIKit
import XCTest

final class TeraMediaBudgetTests: XCTestCase {
  func testStartupLimitsRejectZeroAndEveryMaximumPlusOne() throws {
    let maximum = [8 * 1024 * 1024, 16_000_000, 1024, 32 * 1024 * 1024, 64, 2, 16, 4096]
    func limits(_ values: [Int]) -> TeraMediaPresentationLimits? {
      TeraMediaPresentationLimits(encodedBytes: values[0], sourcePixels: values[1],
                                  thumbnailDimension: values[2], cacheBytes: values[3], cacheEntries: values[4],
                                  workers: values[5], queuedRequests: values[6], visibilityEntries: values[7])
    }
    XCTAssertEqual(limits(maximum), .standard)
    for index in maximum.indices {
      var invalid = maximum
      invalid[index] += 1
      XCTAssertNil(limits(invalid))
      invalid[index] = 0
      XCTAssertNil(limits(invalid))
    }
    let policy = try XCTUnwrap(limits(maximum))
    XCTAssertTrue(policy.accepts(byteCount: maximum[0], width: 4000, height: 4000))
    XCTAssertFalse(policy.accepts(byteCount: maximum[0] + 1, width: 4000, height: 4000))
    XCTAssertFalse(policy.accepts(byteCount: maximum[0], width: 16_000_001, height: 1))
    XCTAssertFalse(policy.accepts(byteCount: 1, width: Int.max, height: Int.max))
  }

  @MainActor
  func testDecodeUsesBoundedRasterOffUIActorAndRejectsCorruption() async throws {
    let renderer = UIGraphicsImageRenderer(size: CGSize(width: 2048, height: 1024), format: {
      let format = UIGraphicsImageRendererFormat()
      format.scale = 1
      return format
    }())
    let bytes = renderer.pngData { context in
      UIColor.green.setFill()
      context.fill(CGRect(x: 0, y: 0, width: 2048, height: 1024))
    }
    let artifact = try XCTUnwrap(TeraVerifiedMediaArtifact(artifactID: String(repeating: "a", count: 64),
                                                           bytes: bytes, byteSize: UInt64(bytes.count), mediaType: "image/png", width: 2048, height: 1024))
    let image = try await TeraMediaThumbnail.prepare(artifact, limits: .standard)
    XCTAssertEqual(image.width, 1024)
    XCTAssertEqual(image.height, 512)
    do {
      _ = try await TeraMediaThumbnail.prepare(TeraScopeFixtures.artifact("a", corrupt: true), limits: .standard)
      XCTFail("Corrupt raster must fail")
    } catch TeraMediaThumbnailFailure.corrupt {}
  }

  func testActualDecoderRejectsEncodedAndDeclaredPixelMaximumPlusOne() async throws {
    let original = try TeraScopeFixtures.artifact("a")
    for (bytes, width, height) in [(Data(repeating: 0, count: 8 * 1024 * 1024 + 1), UInt32(1), UInt32(1)),
                                   (original.bytes, UInt32(16_000_001), UInt32(1))]
    {
      let artifact = try XCTUnwrap(TeraVerifiedMediaArtifact(artifactID: original.artifactID,
                                                             bytes: bytes, byteSize: UInt64(bytes.count), mediaType: "image/png", width: width, height: height))
      do {
        _ = try await TeraMediaThumbnail.prepare(artifact, limits: .standard)
        XCTFail("Resource admission must precede raster decoding")
      } catch TeraMediaThumbnailFailure.resourceLimit {}
    }
  }

  @MainActor
  func testQueueMaxPlusOneAndCancellationKeepPhysicalWorkersBounded() async {
    let queue = TeraMediaWorkQueue(limits: .standard)
    let firstPause = ResourceTestPause()
    let secondPause = ResourceTestPause()
    let first = UUID()
    var started = 0
    XCTAssertTrue(queue.submit(id: first) { started += 1; await firstPause.wait() })
    XCTAssertTrue(queue.submit(id: UUID()) { started += 1; await secondPause.wait() })
    for _ in 0 ..< 16 {
      XCTAssertTrue(queue.submit(id: UUID()) { started += 1 })
    }
    await firstPause.entered.wait()
    await secondPause.entered.wait()
    XCTAssertEqual(started, 2)
    XCTAssertEqual(queue.activeCount, 2)
    XCTAssertEqual(queue.queuedCount, 16)
    XCTAssertFalse(queue.submit(id: UUID()) {})
    queue.cancel(id: first)
    XCTAssertEqual(queue.activeCount, 2)
    XCTAssertFalse(queue.submit(id: UUID()) {})
    queue.cancelAll()
    XCTAssertEqual(queue.activeCount, 2)
    XCTAssertEqual(queue.queuedCount, 0)
    await firstPause.resume.open()
    await secondPause.resume.open()
    await TeraScopeFixtures.eventually { queue.activeCount == 0 }
    XCTAssertEqual(started, 2)
  }

  @MainActor
  func testPresentationCacheEvictsOnlyTransientBytesAtCapacity() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let policy = try XCTUnwrap(TeraMediaPresentationLimits(encodedBytes: 1024, sourcePixels: 16_000_000,
                                                           thumbnailDimension: 1024, cacheBytes: 200, cacheEntries: 3, workers: 1, queuedRequests: 1,
                                                           visibilityEntries: 4096))
    let store = TeraMediaStore(runtimeClient: client, limits: policy)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    for index in 0 ..< 4 {
      var reference = TeraScopeFixtures.reference()
      reference = TeraMediaReference(referenceFingerprint: String(repeating: String(index), count: 64),
                                     url: reference.url, sha256: reference.sha256, mediaType: reference.mediaType,
                                     width: reference.width, height: reference.height, byteSize: reference.byteSize,
                                     alt: reference.alt, verification: reference.verification)
      store.load(media: reference, context: context)
      await TeraScopeFixtures.eventually { store.state(for: reference, context: context) != .loading }
      XCTAssertLessThanOrEqual(store.stateCount, policy.cacheEntries)
      XCTAssertLessThanOrEqual(store.cachedByteCount, policy.cacheBytes)
      XCTAssertGreaterThan(store.cachedByteCount, 0)
    }
    let invalidations = await backend.counts[.invalidate]
    XCTAssertNil(invalidations)
    store.reset()
    XCTAssertEqual(store.cachedByteCount, 0)
    _ = try await client.stop()
  }

  @MainActor
  func testVisibilityCapacityFailsClosedAndExplicitCurrentReferencesCanReturn() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let limits = try XCTUnwrap(TeraMediaPresentationLimits(encodedBytes: 1024,
                                                           sourcePixels: 16_000_000, thumbnailDimension: 1024, cacheBytes: 1024,
                                                           cacheEntries: 64, workers: 2, queuedRequests: 16, visibilityEntries: 2))
    let store = TeraMediaStore(runtimeClient: client, limits: limits)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    let references = (0 ..< 3).map { reference($0, verification: .unavailable) }
    store.reconcileVisibility(previous: Array(references.prefix(2)), current: [], context: context)
    store.reconcileVisibility(previous: [references[0]], current: [], context: context)
    let artifact = try TeraScopeFixtures.artifact("a")
    store.load(media: references[2], context: context)
    await TeraScopeFixtures.eventually { store.state(for: references[2], context: context) == .ready(artifact) }
    store.reconcileVisibility(previous: [references[2]], current: [], context: context)
    for value in references {
      store.load(media: value, context: context)
      store.retry(media: value, context: context)
      XCTAssertEqual(store.state(for: value, context: context), .unavailable)
    }
    let calls = await backend.counts[.media]
    XCTAssertEqual(calls, 1)
    store.reconcileVisibility(previous: [], current: [references[2]], context: context)
    store.load(media: references[2], context: context)
    await TeraScopeFixtures.eventually { store.state(for: references[2], context: context) == .ready(artifact) }
    store.retry(media: references[0], context: context)
    let finalCalls = await backend.counts[.media]
    XCTAssertEqual(finalCalls, 2)
    store.reset()
    _ = try await client.stop()
  }

  @MainActor
  func testPendingStateEntryMaximumPlusOneIsBounded() async throws {
    let backend = try TeraScopeBackend()
    let client = try await TeraScopeFixtures.client(backend)
    let store = TeraMediaStore(runtimeClient: client)
    let context = TeraLocalNetwork.defaultContext(snapshot: TeraScopeFixtures.snapshot())
    for index in 0 ..< 64 {
      store.load(media: reference(index, verification: .pending), context: context)
    }
    XCTAssertEqual(store.stateCount, 64)
    store.load(media: reference(64, verification: .pending), context: context)
    XCTAssertEqual(store.stateCount, 64)
    let calls = await backend.counts[.media]
    XCTAssertNil(calls)
    store.reset()
    _ = try await client.stop()
  }

  private func reference(_ index: Int, verification: TeraMediaVerificationState) -> TeraMediaReference {
    let original = TeraScopeFixtures.reference()
    return TeraMediaReference(referenceFingerprint: String(format: "%064x", index),
                              url: original.url, sha256: original.sha256, mediaType: original.mediaType,
                              width: original.width, height: original.height, byteSize: original.byteSize,
                              alt: original.alt, verification: verification)
  }
}
