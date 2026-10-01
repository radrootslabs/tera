import ImageIO
@testable import TeraApp
import UIKit
import UniformTypeIdentifiers
import XCTest

@MainActor
final class TeraPublicDisclosureTests: XCTestCase {
  func testCameraAndLibraryStripSensitiveMetadataBeforeCreatingPublicMedia() async throws {
    let source = try taggedImage()
    let sourceProperties = try properties(source)
    XCTAssertNotNil(sourceProperties[kCGImagePropertyGPSDictionary], "The fixture must actually contain GPS")
    XCTAssertNotNil(sourceProperties[kCGImagePropertyTIFFDictionary])
    for camera in [false, true] {
      let fixture = try OfflineMediaFixture(image: source, filename: "tagged.jpg", mediaType: "image/jpeg")
      defer { fixture.remove() }
      let coordinator = fixture.coordinator()
      let photos = try await camera ? [coordinator.captureImage()] : coordinator.importImages(limit: 1)
      let photo = try XCTUnwrap(photos.first)
      let bytes = try Data(contentsOf: fixture.roots.stagedBlobsRoot.appendingPathComponent(photo.sha256))
      let output = try properties(bytes)
      XCTAssertNil(output[kCGImagePropertyGPSDictionary])
      XCTAssertNil(output[kCGImagePropertyTIFFDictionary])
      XCTAssertNil(output[kCGImagePropertyIPTCDictionary])
      let exif = output[kCGImagePropertyExifDictionary] as? [CFString: Any] ?? [:]
      XCTAssertNil(exif[kCGImagePropertyExifUserComment])
      XCTAssertNil(bytes.range(of: Data("C117_PRIVATE_CAMERA".utf8)))
      XCTAssertNil(photo.remoteURL, "Local intake does not upload or publish")
      XCTAssertEqual(photo.alt, "")
      XCTAssertEqual(try Data(contentsOf: fixture.roots.cacheRoot.appendingPathComponent("tagged.jpg")), source)
      let transfers = await fixture.transfer.enqueueCount
      XCTAssertEqual(transfers, 0)
    }
  }

  func testLocationIsAbsentByDefaultAndOnlyManualTextSurvivesSaveAndRestart() async throws {
    let fixture = try OfflineMediaFixture()
    defer { fixture.remove() }
    let client = TeraRuntimeClient.production()
    let signer = ComposerForbiddenSigner()
    let configuration = fixture.configuration(signer)
    let snapshot = try await client.start(configuration: configuration)
    var saved: [TeraComposerDraft] = []
    for type in TeraAddCommandType.allCases {
      let store = TeraAddStore(runtimeClient: client, initialType: type)
      store.configure(snapshot: snapshot)
      await store.start()
      XCTAssertNil(store.form.location)
      store.updateForm(\.content, "local editing")
      if type == .createEvent || type == .createFoodAvailability {
        store.updateForm(\.location, "North side of the public market")
      }
      await store.save()
      let draft = try XCTUnwrap(store.savedComposer)
      XCTAssertEqual(draft.form.location, store.form.location)
      saved.append(draft)
      store.stop()
    }
    _ = try await client.stop()
    _ = try await client.start(configuration: configuration)
    for draft in saved {
      let restored = try await client.loadComposer(scope: draft.scope, id: draft.id)
      XCTAssertEqual(restored, draft)
    }
    let signs = await signer.requests
    XCTAssertEqual(signs, 0)
    _ = try await client.stop()
  }

  private func properties(_ data: Data) throws -> [CFString: Any] {
    let source = try XCTUnwrap(CGImageSourceCreateWithData(data as CFData, nil))
    return try XCTUnwrap(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
  }

  private func taggedImage() throws -> Data {
    let image = UIGraphicsImageRenderer(size: CGSize(width: 2, height: 2)).image { context in
      UIColor.green.setFill()
      context.fill(CGRect(x: 0, y: 0, width: 2, height: 2))
    }
    let output = NSMutableData()
    let destination = try XCTUnwrap(CGImageDestinationCreateWithData(output, UTType.jpeg.identifier as CFString, 1, nil))
    let properties: [CFString: Any] = [
      kCGImagePropertyGPSDictionary: [kCGImagePropertyGPSLatitude: 12.3456, kCGImagePropertyGPSLatitudeRef: "N"],
      kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFMake: "C117_PRIVATE_CAMERA"],
      kCGImagePropertyExifDictionary: [kCGImagePropertyExifUserComment: "C117_PRIVATE_CAMERA"],
      kCGImagePropertyIPTCDictionary: [kCGImagePropertyIPTCCaptionAbstract: "C117_PRIVATE_CAMERA"],
    ]
    try CGImageDestinationAddImage(destination, XCTUnwrap(image.cgImage), properties as CFDictionary)
    XCTAssertTrue(CGImageDestinationFinalize(destination))
    return output as Data
  }
}
