import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraIdentityCancellationTests: XCTestCase {
    func testCancelledHostImportCannotInstallAfterSuccessfulPresenceCallback() async throws {
        let secure = InMemorySecureStore()
        let presence = IdentityCancellationPresence()
        let custody = try RadrootsIdentityCustody(
          configuration: RadrootsIdentityCustodyConfiguration(namespace: "radroots_identity_v1", secretPolicy: .secureLocalSecret),
          secureStore: secure, metadataStore: InMemoryIdentityMetadataStore(), userPresence: presence
        )
        let prefix = "tera.identity.cancellation.\(UUID().uuidString.lowercased())"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: prefix))
        defer { UserDefaults(suiteName: prefix)?.removePersistentDomain(forName: prefix) }
        let store = TeraIdentityStore(custody: custody, secureStore: secure, servicePrefix: prefix, userDefaults: defaults)
        let task = Task {
            try await store.importIdentity(RadrootsIdentitySecretMaterial(rawRepresentation: Data(repeating: 1, count: 32)))
        }
        for await _ in presence.entered {
          break
        }
        task.cancel()
        await presence.release()
        do {
            _ = try await task.value
            XCTFail("Cancellation must reject a later successful presence callback")
        } catch {
            XCTAssertEqual(error as? RadrootsIdentityCustodyError, .cancelled)
        }
        let state = await store.snapshot()
        XCTAssertEqual(state.state, .absent)
        XCTAssertFalse(try secure.contains(RadrootsSecureStoreKey(namespace: "radroots_identity_v1", name: "active_secret_v1")))
    }

    func testCustodyErrorPresentationDoesNotExposeSecretOrInternalText() {
        let sentinel = "synthetic-secret-sentinel"
        let error = TeraIdentityStoreError.custody(sentinel)
        XCTAssertFalse(error.localizedDescription.contains(sentinel))
        XCTAssertEqual(error.localizedDescription, "The local identity needs attention before Tera can continue.")
    }
}

private actor IdentityCancellationPresence: RadrootsUserPresence {
    nonisolated let entered: AsyncStream<Void>
    private let signal: AsyncStream<Void>.Continuation
    private var pending: CheckedContinuation<Void, Never>?
    init() {
      (entered, signal) = AsyncStream.makeStream()
    }

    func currentStatus() async throws -> RadrootsUserPresenceStatus {
      .unavailable
    }

    func release() {
        pending?.resume()
        pending = nil
    }

    func verify(_ request: RadrootsUserPresenceRequest) async throws -> RadrootsUserPresenceResult {
        await withCheckedContinuation { continuation in
            pending = continuation
            signal.yield(())
        }
        return RadrootsUserPresenceResult(policy: request.policy, verified: true)
    }
}
