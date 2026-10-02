import CoreFoundation
import Foundation
@testable import TeraApp
import XCTest

final class TeraPrivacyPackagingTests: XCTestCase {
  func testInstalledBundleContainsLinkedPublicContentAndReviewedReasonsWithoutTracking() throws {
    let url = try XCTUnwrap(Bundle.main.url(forResource: "PrivacyInfo", withExtension: "xcprivacy"))
    let value = try PropertyListSerialization.propertyList(from: Data(contentsOf: url), format: nil)
    let manifest = try XCTUnwrap(value as? [String: Any])
    let tracking = try XCTUnwrap(manifest["NSPrivacyTracking"] as? NSNumber)
    XCTAssertEqual(CFGetTypeID(tracking), CFBooleanGetTypeID())
    XCTAssertFalse(tracking.boolValue)
    XCTAssertEqual(manifest["NSPrivacyTrackingDomains"] as? [String], [])
    let dataTypes = try XCTUnwrap(manifest["NSPrivacyCollectedDataTypes"] as? [[String: Any]])
    XCTAssertEqual(dataTypes.compactMap { $0["NSPrivacyCollectedDataType"] as? String }, [
      "NSPrivacyCollectedDataTypeName", "NSPrivacyCollectedDataTypeUserID",
      "NSPrivacyCollectedDataTypePhysicalAddress", "NSPrivacyCollectedDataTypePhotosorVideos",
      "NSPrivacyCollectedDataTypeOtherUserContent",
    ])
    for type in dataTypes {
      XCTAssertEqual(type["NSPrivacyCollectedDataTypeLinked"] as? Bool, true)
      XCTAssertEqual(type["NSPrivacyCollectedDataTypeTracking"] as? Bool, false)
      XCTAssertEqual(type["NSPrivacyCollectedDataTypePurposes"] as? [String], ["NSPrivacyCollectedDataTypePurposeAppFunctionality"])
    }
    let apis = try XCTUnwrap(manifest["NSPrivacyAccessedAPITypes"] as? [[String: Any]])
    XCTAssertEqual(apis.compactMap { $0["NSPrivacyAccessedAPIType"] as? String }, [
      "NSPrivacyAccessedAPICategoryFileTimestamp", "NSPrivacyAccessedAPICategoryDiskSpace", "NSPrivacyAccessedAPICategoryUserDefaults",
    ])
    XCTAssertEqual(apis.compactMap { $0["NSPrivacyAccessedAPITypeReasons"] as? [String] }, [["C617.1"], ["E174.1"], ["CA92.1"]])
  }

  func testInstalledPurposeAndSupportContactDescribeCurrentPublicPosting() throws {
    XCTAssertEqual(Bundle.main.object(forInfoDictionaryKey: "NSCameraUsageDescription") as? String,
                   "Tera uses the camera only when you choose to add a photo to a public post.")
    XCTAssertEqual(Bundle.main.object(forInfoDictionaryKey: "TeraSupportEmail") as? String, "support@radroots.org")
    let mail = try XCTUnwrap(TeraSupportContact.packagedMailURL)
    XCTAssertEqual(mail.absoluteString, "mailto:support@radroots.org")
  }

  func testContactRefusesUnreviewedOrInjectedRecipients() {
    for email: String? in [nil, "", "operator@example.invalid", "support@radroots.org?subject=private", " support@radroots.org"] {
      XCTAssertNil(TeraSupportContact.mailURL(email: email))
    }
    XCTAssertEqual(TeraSupportContact.mailURL(email: "support@radroots.org")?.absoluteString, "mailto:support@radroots.org")
  }
}
