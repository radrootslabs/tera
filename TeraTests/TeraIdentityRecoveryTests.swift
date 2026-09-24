import Foundation
import RadrootsKit
@testable import TeraApp
import UIKit
import XCTest

final class TeraIdentityRecoveryTests: XCTestCase {
    func testMetadataMismatchRetainsLegacyAndDoesNotInstallReplacement() async throws {
        let fixture = try IdentityRecoveryFixture()
        defer { fixture.remove() }
        try fixture.legacy(publicKey: String(repeating: "ab", count: 32))
        let store = try fixture.store()
        do {
            _ = try await store.recover()
            XCTFail("Mismatched identity must not migrate")
        } catch {
            XCTAssertEqual(error as? TeraIdentityStoreError, .custody("identity.inconsistent_state"))
        }
        let state = await store.snapshot()
        XCTAssertEqual(state.state, .absent)
        XCTAssertTrue(try fixture.secure.contains(fixture.legacyKey))
        XCTAssertNotNil(fixture.defaults.data(forKey: fixture.metadataKey))
    }

    func testInterruptedLegacyCleanupRetainsOriginalIdentityAcrossRestart() async throws {
        let fixture = try IdentityRecoveryFixture()
        defer { fixture.remove() }
        let store = try fixture.store()
        try fixture.legacy()
        fixture.secure.failLegacyDeletion = true
        do {
            _ = try await store.recover()
            XCTFail("Interrupted cleanup must require recovery")
        } catch {
            XCTAssertEqual(error as? TeraIdentityStoreError, .custody("identity.recovery_required"))
        }
        let installed = await store.snapshot()
        let original = try XCTUnwrap(installed.publicKeyHex)
        XCTAssertEqual(installed.state, .unlocked)
        fixture.secure.failLegacyDeletion = false
        fixture.metadata(publicKey: original)
        let restarted = try fixture.store()
        let pending = try await restarted.loadAndMigrate()
        XCTAssertEqual(pending.state, .recoveryRequired)
        XCTAssertEqual(pending.publicKeyHex, original)
        XCTAssertNil(pending.signerGeneration)
        let recovered = try await restarted.recover()
        XCTAssertEqual(recovered.publicKeyHex, original)
        XCTAssertEqual(recovered.state, .locked)
        XCTAssertFalse(try fixture.secure.contains(fixture.legacyKey))
        XCTAssertNil(fixture.defaults.data(forKey: fixture.metadataKey))
        let unlocked = try await restarted.unlock()
        XCTAssertEqual(unlocked.publicKeyHex, original)
    }

    func testMetadataOnlyReplayValidatesInstalledKeyBeforeCleanup() async throws {
        let fixture = try IdentityRecoveryFixture()
        defer { fixture.remove() }
        let store = try fixture.store()
        let installed = try await store.importIdentity(fixture.material())
        let original = try XCTUnwrap(installed.publicKeyHex)
        fixture.metadata(publicKey: String(repeating: "ab", count: 32))
        do {
            _ = try await store.recover()
            XCTFail("Mismatched metadata cannot be discarded")
        } catch {
            XCTAssertEqual(error as? TeraIdentityStoreError, .custody("identity.inconsistent_state"))
        }
        XCTAssertNotNil(fixture.defaults.data(forKey: fixture.metadataKey))
        fixture.metadata(publicKey: original)
        let restarted = try fixture.store()
        let recovered = try await restarted.recover()
        XCTAssertEqual(recovered.publicKeyHex, original)
        XCTAssertNil(fixture.defaults.data(forKey: fixture.metadataKey))
    }

    func testPendingLegacyRecoveryRefusesStaleCreateAndImport() async throws {
        let fixture = try IdentityRecoveryFixture()
        defer { fixture.remove() }
        try fixture.legacy()
        let store = try fixture.store()
        for create in [true, false] {
            do {
                if create {
                  _ = try await store.create()
                } else {
                  _ = try await store.importIdentity(fixture.material())
                }
                XCTFail("Pending legacy identity must not be replaced")
            } catch {
                XCTAssertEqual(error as? TeraIdentityStoreError, .custody("identity.recovery_required"))
            }
        }
        let state = await store.snapshot()
        XCTAssertEqual(state.state, .absent)
        XCTAssertTrue(try fixture.secure.contains(fixture.legacyKey))
    }

    func testLostOrCorruptInstalledKeyNeverCreatesReplacement() async throws {
        for missing in [true, false] {
            let fixture = try IdentityRecoveryFixture()
            defer { fixture.remove() }
            let store = try fixture.store()
            let original = try await store.importIdentity(fixture.material())
            await store.lock()
            let key = RadrootsSecureStoreKey(namespace: "radroots_identity_v1", name: "active_secret_v1")
            if missing {
              try fixture.secure.delete(key)
            } else {
              try fixture.secure.put(Data(repeating: 0, count: 32), for: key, policy: .secureLocalSecret)
            }
            let restarted = try fixture.store()
            do {
                _ = try await restarted.unlock()
                XCTFail("Unavailable original key must not unlock")
            } catch {
                XCTAssertTrue(error is RadrootsIdentityCustodyError)
            }
            do {
                _ = try await restarted.create()
                XCTFail("Existing identity must not be silently replaced")
            } catch {
                XCTAssertTrue(error is RadrootsIdentityCustodyError)
            }
            let after = await restarted.snapshot()
            XCTAssertEqual(after.publicKeyHex, original.publicKeyHex)
            XCTAssertNil(after.signerGeneration)
            XCTAssertEqual(try fixture.secure.get(key), missing ? nil : Data(repeating: 0, count: 32))
        }
    }

    func testProtectedDataAndDeniedPresenceRetainInstalledKey() async throws {
        let fixture = try IdentityRecoveryFixture()
        defer { fixture.remove() }
        let original = try await fixture.store().importIdentity(fixture.material())
        let protected = try fixture.store(protected: true)
        let state = try await protected.loadAndMigrate()
        XCTAssertEqual(state.state, .protectedDataUnavailable)
        XCTAssertEqual(state.publicKeyHex, original.publicKeyHex)
        let denied = try fixture.store(presence: RefusingIdentityPresence())
        do {
            _ = try await denied.unlock()
            XCTFail("Presence denial must not unlock")
        } catch {
            XCTAssertTrue(error is RadrootsIdentityCustodyError)
        }
        let after = await denied.snapshot()
        XCTAssertEqual(after.state, .locked)
        XCTAssertEqual(after.publicKeyHex, original.publicKeyHex)
    }

    @MainActor
    func testImportTeardownClearsSecretAndRejectsLateSubmit() {
        var submissions = 0
        let coordinator = TeraSecureIdentityImportField.Coordinator { _ in submissions += 1 }
        let field = UITextField()
        field.text = String(repeating: "01", count: 32)
        field.delegate = coordinator
        let error = UILabel()
        error.text = "Validation failed"
        coordinator.field = field
        coordinator.errorLabel = error
        TeraSecureIdentityImportField.dismantleUIView(UIView(), coordinator: coordinator)
        coordinator.submitIdentity()
        XCTAssertEqual(field.text ?? "", "")
        XCTAssertNil(field.delegate)
        XCTAssertNil(error.text)
        XCTAssertNil(coordinator.field)
        XCTAssertEqual(submissions, 0)
    }
}

private struct IdentityRecoveryFixture {
    let secure = LegacyDeletionFaultStore()
    let metadataStore = InMemoryIdentityMetadataStore()
    let prefix = "tera.identity.recovery.\(UUID().uuidString.lowercased())"
    let defaults: UserDefaults
    let legacyKey = RadrootsSecureStoreKey(namespace: "nostr_identity", name: "selected_secret_hex")
    var metadataKey: String {
      "field_ios.identity.public_metadata.\(prefix)"
    }

    init() throws {
      defaults = try XCTUnwrap(UserDefaults(suiteName: prefix))
    }

    func remove() {
      defaults.removePersistentDomain(forName: prefix)
    }

    func material() throws -> RadrootsIdentitySecretMaterial {
        try RadrootsIdentitySecretMaterial(rawRepresentation: Data(repeating: 1, count: 32))
    }

    func legacy(publicKey: String? = nil) throws {
        try secure.put(Data(String(repeating: "01", count: 32).utf8), for: legacyKey, policy: .secureLocalSecret)
        if let publicKey {
          metadata(publicKey: publicKey)
        }
    }

    func metadata(publicKey: String) {
        defaults.set(try? JSONSerialization.data(withJSONObject: [
          "selectedIdentityId": "legacy", "publicKeyHex": publicKey,
          "publicKeyNpub": "legacy", "updatedAtUnix": 1,
        ]), forKey: metadataKey)
    }

    func store(
      presence: any RadrootsUserPresence = AllowingUserPresence(),
      protected: Bool = false
    ) throws -> TeraIdentityStore {
        let custody = try RadrootsIdentityCustody(
          configuration: RadrootsIdentityCustodyConfiguration(namespace: "radroots_identity_v1", secretPolicy: .secureLocalSecret),
          secureStore: secure, metadataStore: metadataStore, userPresence: presence,
          protectedData: RadrootsProtectedDataProvider { protected ? .unavailable : .available }
        )
        let suiteName = prefix
        let custodyDefaults = try XCTUnwrap(UserDefaults(suiteName: suiteName))
        return TeraIdentityStore(custody: custody, secureStore: secure, servicePrefix: suiteName, userDefaults: custodyDefaults)
    }
}

private final class LegacyDeletionFaultStore: RadrootsSecureStore, @unchecked Sendable {
    private let backing = InMemorySecureStore()
    private let lock = NSLock()
    private var fails = false
    var failLegacyDeletion: Bool {
        get { lock.withLock { fails } }
        set { lock.withLock { fails = newValue } }
    }

    func put(_ value: Data, for key: RadrootsSecureStoreKey, policy: RadrootsSecretAccessPolicy) throws {
        try backing.put(value, for: key, policy: policy)
    }

    func get(_ key: RadrootsSecureStoreKey) throws -> Data? {
      try backing.get(key)
    }

    func contains(_ key: RadrootsSecureStoreKey) throws -> Bool {
      try backing.contains(key)
    }

    func delete(_ key: RadrootsSecureStoreKey) throws {
        if key.namespace == "nostr_identity", failLegacyDeletion {
          throw TeraIdentityStoreError.unavailable
        }
        try backing.delete(key)
    }

    func deleteNamespace(_ namespace: String) throws {
      try backing.deleteNamespace(namespace)
    }
}

private struct RefusingIdentityPresence: RadrootsUserPresence {
    func currentStatus() async throws -> RadrootsUserPresenceStatus {
      .unavailable
    }

    func verify(_ request: RadrootsUserPresenceRequest) async throws -> RadrootsUserPresenceResult {
        RadrootsUserPresenceResult(policy: request.policy, verified: false)
    }
}
