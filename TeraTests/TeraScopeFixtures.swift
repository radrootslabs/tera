import Foundation
@testable import TeraApp
import XCTest

enum TeraScopeFixtures {
  static func snapshot(
    account: String = "a", relay: String = "first", evidence: TeraBlossomEvidence? = nil
  ) -> TeraRuntimeSnapshot {
    TeraRuntimeSnapshot(
      identity: TeraRuntimeIdentity(publicKeyHex: String(repeating: account, count: 64), hostSignerConfigured: true),
      relay: TeraRelayStatus(
        profile: "simulator", state: "configured", readAvailability: "unobserved", writeAvailability: "unobserved",
        relays: [TeraRelayEndpointStatus(
          url: "wss://\(relay).example", access: .readWrite, readState: "unobserved", writeState: "unobserved",
          readLastAttemptUnixMilliseconds: nil, writeLastAttemptUnixMilliseconds: nil,
          readNextAttemptUnixMilliseconds: nil, writeNextAttemptUnixMilliseconds: nil
        )]
      ),
      blossomConfiguration: TeraBlossomConfigurationStatus(
        schemaVersion: 1, hostKind: "simulator", endpointAuthority: "loopback_development",
        primaryOrigin: "http://127.0.0.1:3000", fallbackOrigins: [], configFingerprint: relay
      ),
      blossomEvidence: evidence, crateName: "tera_ffi", crateVersion: "0.1.0-alpha", isClosed: false
    )
  }

  static func evidence(observedAt: UInt64) -> TeraBlossomEvidence {
    TeraBlossomEvidence(
      schemaVersion: 2, origin: "http://127.0.0.1:3000", configFingerprint: "first",
      state: "reachable", lastSuccessfulState: "probe", transportSecurity: "loopback_plaintext",
      observedAtUnixMilliseconds: observedAt, httpStatus: 200, errorCode: nil, serverErrorCode: nil,
      errorPhase: nil, retryable: false, possibleOrphan: false, attempts: 1
    )
  }

  static func card(_ id: String) -> TeraTodayCard {
    TeraTodayCard(
      id: id, type: .update, sourceEventID: id, sourceAddress: nil,
      authorPublicKey: String(repeating: "a", count: 64), contractID: "test.update",
      title: nil, content: id, authoredAtUnixSeconds: 1, effectiveAtUnixSeconds: 1,
      eventStartUnixSeconds: nil, eventEndUnixSeconds: nil, location: nil,
      priceAmount: nil, priceCurrency: nil, priceUnit: nil, quantity: nil, foodSummary: nil,
      foodPublishedAtUnixSeconds: nil, foodStatus: nil, contextRank: 1, inclusionReason: "local",
      media: [], lifecycle: .active, rankDigest: nil, authorProfile: nil, thread: [],
      localOperationID: nil, localOperationState: nil
    )
  }

  static func draft(_ text: String, revision: UInt64 = 1) -> TeraDraftStatus {
    var form = TeraAddForm.empty(.createUpdate)
    form.content = text
    return TeraDraftStatus(
      id: "draft", revision: revision, authorPublicKey: String(repeating: "a", count: 64),
      kind: .add, commandType: .createUpdate, form: form, state: .draft,
      cardID: "card", operationID: nil, createdAtUnixMilliseconds: 1,
      updatedAtUnixMilliseconds: revision, media: [], settlement: nil, isRevision: false
    )
  }

  static func reference() -> TeraMediaReference {
    TeraMediaReference(
      referenceFingerprint: String(repeating: "e", count: 64), url: "https://media.example/image.png",
      sha256: String(repeating: "b", count: 64), mediaType: "image/png", width: 1, height: 1,
      byteSize: 68, alt: "Test pixel", verification: .unavailable
    )
  }

  static func artifact(_ id: String, corrupt: Bool = false) throws -> TeraVerifiedMediaArtifact {
    let pixel = Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jWZkAAAAASUVORK5CYII=") ?? Data()
    let bytes = corrupt ? Data([0, 1, 2]) : pixel
    return try XCTUnwrap(TeraVerifiedMediaArtifact(
      artifactID: String(repeating: id, count: 64), bytes: bytes,
      byteSize: UInt64(bytes.count), mediaType: "image/png", width: 1, height: 1
    ))
  }

  static func failure() -> TeraRuntimeFailure {
    .local(operation: "test.scope", code: "test.scope.failed", safeMessage: "A controlled request failed.")
  }

  @MainActor
  static func eventually(_ condition: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async {
    let deadline = ContinuousClock.now.advanced(by: .seconds(5))
    while !condition(), ContinuousClock.now < deadline {
      await Task.yield()
    }
    XCTAssertTrue(condition(), file: file, line: line)
  }

  @MainActor
  static func client(_ backend: TeraScopeBackend) async throws -> TeraRuntimeClient {
    let configuration = TeraRuntimeClientTests().makeConfiguration(generation: "51")
    let client = TeraRuntimeClient { _ in
      await TeraRuntimeBackendStart(backend: backend, snapshot: backend.value)
    }
    _ = try await client.start(configuration: configuration)
    return client
  }
}
