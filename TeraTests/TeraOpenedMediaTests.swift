import Foundation
@testable import TeraApp
import XCTest

final class TeraOpenedMediaTests: XCTestCase {
  func testConcurrentRepeatedCloseLeavesOnlyTheUsableRustOwner() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let original = try fixture.original()
    let descriptor = original.fileDescriptor
    let probe = try TeraOpenFileProbe(url: fixture.root.appendingPathComponent("original.png"))
    let opened = try TeraOpenedMedia(
      handles: [TeraPreparedMediaHandle(media: fixture.media, fileDescriptor: UInt64(descriptor))],
      files: [original]
    )
    XCTAssertEqual(probe.descriptorCount, 2)
    await TeraOpenedMediaCloseFixture.closeConcurrently(opened)
    XCTAssertFalse(probe.owns(descriptor))
    XCTAssertEqual(probe.descriptorCount, 1)
    let saved = try await fixture.save(runtime, handle: XCTUnwrap(opened.handles.first))
    XCTAssertEqual(saved.form?.media.first?.sha256, fixture.media.sha256)
    _ = try await runtime.shutdown()
  }

  func testDeinitClosesTheOriginalAndReleasesTheLastRustHandle() throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let original = try fixture.original()
    let descriptor = original.fileDescriptor
    let probe = try TeraOpenFileProbe(url: fixture.root.appendingPathComponent("original.png"))
    var opened: TeraOpenedMedia? = try TeraOpenedMedia(
      handles: [TeraPreparedMediaHandle(media: fixture.media, fileDescriptor: UInt64(descriptor))],
      files: [original]
    )
    weak let weakOwner = opened
    XCTAssertEqual(opened?.handles.count, 1)
    XCTAssertEqual(probe.descriptorCount, 2)
    opened = nil
    XCTAssertNil(weakOwner)
    XCTAssertFalse(probe.owns(descriptor))
    XCTAssertEqual(probe.descriptorCount, 0)
  }

  func testProductionPartialOpenFailureReleasesBothNativeAndRustResources() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let coordinator = fixture.coordinator(transfer: BackgroundTransferHarness())
    let probe = try TeraOpenFileProbe(url: fixture.stagedFileURL)
    var control: TeraOpenedMedia? = try await coordinator.open([fixture.media])
    XCTAssertEqual(probe.descriptorCount, 2)
    control?.close()
    control = nil
    XCTAssertEqual(probe.descriptorCount, 0)
    let missing = missingMedia(fixture.media)
    for _ in 0 ..< 16 {
      do {
        let unexpected = try await coordinator.open([fixture.media, missing])
        unexpected.close()
        XCTFail("The second staged file does not exist")
      } catch {}
      XCTAssertEqual(probe.descriptorCount, 0)
    }
  }

  private func missingMedia(_ media: TeraPreparedMedia) -> TeraPreparedMedia {
    let missing = String(repeating: "f", count: 64)
    return TeraPreparedMedia(
      opaqueReference: "media:\(missing)", remoteURL: nil, sha256: missing,
      mediaType: media.mediaType, byteSize: media.byteSize, width: media.width,
      height: media.height, alt: media.alt, preparedAtUnixSeconds: media.preparedAtUnixSeconds
    )
  }
}
