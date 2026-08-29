import Foundation
@testable import RadrootsApp
import RadrootsKit
import XCTest

final class RadrootsLifecycleTests: XCTestCase {
  func testBackgroundEventsCompleteExactlyOnceBeforeAndAfterAttachment() async {
    let router = RadrootsBackgroundEventRouter()
    let first = CompletionProbe()

    await router.handle(
      identifier: "org.radroots.tests.background.transfer",
      completion: RadrootsCompletionOnce { first.increment() }
    )
    XCTAssertEqual(first.value(), 0)

    await router.attach(identifier: "org.radroots.tests.background.transfer") { _, completion in
      completion()
      completion()
    }
    XCTAssertEqual(first.value(), 1)

    let mismatched = CompletionProbe()
    await router.handle(
      identifier: "org.radroots.tests.background.other",
      completion: RadrootsCompletionOnce { mismatched.increment() }
    )
    XCTAssertEqual(mismatched.value(), 1)
  }

  func testBackgroundEventQueueIsBoundedAndShutdownCompletesPendingEvents() async {
    let router = RadrootsBackgroundEventRouter()
    let probes = (0 ..< 12).map { _ in CompletionProbe() }

    for probe in probes {
      await router.handle(
        identifier: "org.radroots.tests.background.transfer",
        completion: RadrootsCompletionOnce { probe.increment() }
      )
    }

    XCTAssertEqual(probes.filter { $0.value() == 1 }.count, 4)
    await router.detachAndCompletePending()
    XCTAssertTrue(probes.allSatisfy { $0.value() == 1 })
  }

  func testLifecycleBridgeRunsRegisteredShutdownOnlyOnce() async {
    let bridge = RadrootsLifecycleBridge()
    let shutdowns = CompletionProbe()
    await bridge.register { shutdowns.increment() }

    await bridge.requestShutdown()
    await bridge.requestShutdown()

    XCTAssertEqual(shutdowns.value(), 1)
  }

  func testProtectedDataMonitorReflectsTransitions() {
    let monitor = RadrootsProtectedDataMonitor(available: false)
    XCTAssertFalse(monitor.isAvailable())

    monitor.update(available: true)

    XCTAssertTrue(monitor.isAvailable())
  }

  func testDiagnosticsAreBoundedAndExcludeSensitiveRuntimeValues() async throws {
    let roots = try makeRoots()
    defer { try? FileManager.default.removeItem(at: roots.dataRoot.deletingLastPathComponent()) }
    let coordinator = RadrootsLifecycleCoordinator.testing(roots: roots, capacity: 16)
    let secret = "nsec1diagnosticcanary"
    let privateKey = String(repeating: "ab", count: 32)
    let privatePath = "/Users/private/radroots/private.json"

    for index in 0 ..< 40 {
      await coordinator.record(
        "ios.lifecycle.test_event",
        fields: [
          "attempt": "\(index)",
          "content": secret,
          "private_key": privateKey,
          "source": privatePath,
        ]
      )
    }
    let export = try await coordinator.prepareDiagnostics(
      snapshot: makeSnapshot(privateKey: privateKey),
      appVersion: "0.1.0-alpha",
      appBuild: "1",
      phase: "running"
    )
    let data = try Data(contentsOf: export.fileURL)
    let text = try XCTUnwrap(String(data: data, encoding: .utf8))
    let object = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
    let records = try XCTUnwrap(object["records"] as? [[String: Any]])

    XCTAssertEqual(object["schema"] as? String, "radroots.ios.diagnostics.v1")
    XCTAssertEqual(records.count, 16)
    XCTAssertLessThanOrEqual(data.count, 256 * 1024)
    XCTAssertTrue(text.contains("[redacted]"))
    XCTAssertFalse(text.contains(secret))
    XCTAssertFalse(text.contains(privateKey))
    XCTAssertFalse(text.contains(privatePath))
    XCTAssertFalse(text.contains("password@example.test"))

    await coordinator.releaseDiagnostics(export)
    XCTAssertFalse(FileManager.default.fileExists(atPath: export.fileURL.path))
  }

  private func makeRoots() throws -> RadrootsAppleFileRoots {
    let base = FileManager.default.temporaryDirectory
      .appendingPathComponent(
        "radroots-ios-lifecycle-tests-\(UUID().uuidString)", isDirectory: true
      )
    return try RadrootsAppleFileRoots(
      appIdentifier: "org.radroots.tests",
      dataRoot: base.appendingPathComponent("data", isDirectory: true),
      cacheRoot: base.appendingPathComponent("cache", isDirectory: true),
      temporaryRoot: base.appendingPathComponent("temporary", isDirectory: true)
    )
  }

  private func makeSnapshot(privateKey: String) -> RadrootsRuntimeSnapshot {
    RadrootsRuntimeSnapshot(
      identity: RadrootsRuntimeIdentity(
        publicKeyHex: privateKey,
        hostSignerConfigured: true
      ),
      relay: RadrootsRelayStatus(
        profile: "simulator",
        state: "configured",
        readAvailability: "available",
        writeAvailability: "available",
        relays: [
          RadrootsRelayEndpointStatus(
            url: "wss://user:password@example.test",
            access: .readWrite,
            readState: "available",
            writeState: "available",
            readLastAttemptUnixMilliseconds: nil,
            writeLastAttemptUnixMilliseconds: nil,
            readNextAttemptUnixMilliseconds: nil,
            writeNextAttemptUnixMilliseconds: nil
          ),
        ]
      ),
      blossomConfiguration: nil,
      blossomEvidence: nil,
      crateName: "radroots_mobile_ffi",
      crateVersion: "0.1.0-alpha",
      isClosed: false
    )
  }
}

private final class CompletionProbe: @unchecked Sendable {
  private let lock = NSLock()
  private var count = 0

  func increment() {
    lock.withLock { count += 1 }
  }

  func value() -> Int {
    lock.withLock { count }
  }
}
