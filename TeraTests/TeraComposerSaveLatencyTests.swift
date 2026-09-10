import Foundation
@testable import TeraApp
import XCTest

@MainActor
final class TeraComposerSaveLatencyTests: XCTestCase {
  func testTextOnlyExplicitSaveMeasuresActualDurableReceiptsAndReopen() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let signer = ComposerForbiddenSigner()
    let configuration = configuration(fixture, signer: signer)
    let client = TeraRuntimeClient.production()
    let snapshot = try await client.start(configuration: configuration)
    let store = TeraAddStore(runtimeClient: client)
    store.configure(snapshot: snapshot)
    await store.start()
    store.updateForm(\.content, "warm text-only save")
    await store.save()
    var previous = try XCTUnwrap(store.savedComposer)
    var samples: [Double] = []
    let clock = ContinuousClock()
    for index in 0 ..< 24 {
      let content = "bounded text-only sample \(index)"
      store.updateForm(\.content, content)
      let started = clock.now
      await store.save()
      let duration = started.duration(to: clock.now).components
      samples.append(Double(duration.seconds) * 1000 + Double(duration.attoseconds) / 1e15)
      let saved = try XCTUnwrap(store.savedComposer)
      XCTAssertEqual(store.composerState, .saved)
      XCTAssertEqual(saved.form.content, content)
      XCTAssertTrue(saved.form.media.isEmpty)
      XCTAssertEqual(saved.id, previous.id)
      XCTAssertEqual(saved.revision, previous.revision + 1)
      previous = saved
    }
    store.stop()
    _ = try await client.stop()
    _ = try await client.start(configuration: configuration)
    let reopened = try await client.loadComposer(scope: previous.scope, id: previous.id)
    XCTAssertEqual(reopened, previous)
    _ = try await client.stop()
    let signingRequests = await signer.requests
    XCTAssertEqual(signingRequests, 0)
    try record(samples)
  }

  private func record(_ samples: [Double]) throws {
    XCTAssertEqual(samples.count, 24)
    XCTAssertTrue(samples.allSatisfy { $0.isFinite && $0 >= 0 })
    let ordered = samples.sorted()
    #if targetEnvironment(simulator)
      let platform = "ios_simulator"
    #else
      let platform = "ios_physical_device"
    #endif
    let value: [String: Any] = try [
      "schema": "tera.explicit-save-latency.v1",
      "platform": platform,
      "os": ProcessInfo.processInfo.operatingSystemVersionString,
      "sample_count": samples.count,
      "warmup_count": 1,
      "samples_ms": samples,
      "p50_ms": ordered[ordered.count / 2],
      "p95_ms": ordered[Int(ceil(Double(ordered.count) * 0.95)) - 1],
      "max_ms": XCTUnwrap(ordered.last),
      "measurement": "Explicit Add Save through production FFI to its acknowledged receipt; final revision reopened.",
    ]
    let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
    attachment.name = "tera-explicit-save-latency.json"
    attachment.lifetime = .keepAlways
    add(attachment)
    try print("TERA_EXPLICIT_SAVE_LATENCY \(XCTUnwrap(String(data: data, encoding: .utf8)))")
  }

  private func configuration(_ fixture: MediaOwnershipFixture, signer: ComposerForbiddenSigner) -> TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: fixture.root.path,
      publicKeyHex: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
      sourceGenerationHex: String(repeating: "04", count: 32),
      sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
      protectedData: .available, networkProfile: .publicNetwork,
      writableRelays: ["wss://relay.example"], blossom: nil,
      app: TeraRuntimeAppMetadata(bundleIdentifier: "test.composer-latency", version: "1", buildNumber: "1", buildSHA: nil),
      signerGeneration: "composer-latency", signer: signer, adoptBootstrapSettings: false
    )
  }
}
