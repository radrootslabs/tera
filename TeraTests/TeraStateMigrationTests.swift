import Foundation
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraStateMigrationTests: XCTestCase {
    func testRelayValidationMatchesRuntimeProfiles() throws {
        XCTAssertEqual(
          try TeraNetworkValidator.relays(
            ["wss://radroots.org", "WSS://WRITE.EXAMPLE:443/"],
            profile: .publicNetwork
          ),
          ["wss://write.example"]
        )
        XCTAssertEqual(
          try TeraNetworkValidator.relays(
            ["ws://127.0.0.1:7447"],
            profile: .simulator
          ),
          ["ws://127.0.0.1:7447"]
        )
        for accepted in [
          "ws://10.0.0.5:7447",
          "ws://172.16.0.5:7447",
          "ws://172.31.255.254:7447",
          "ws://192.168.0.5:7447",
          "ws://[fc00::5]:7447",
          "ws://[fd00::5]:7447",
          "wss://10.0.0.5:7447",
        ] {
            XCTAssertNoThrow(
              try TeraNetworkValidator.relays([accepted], profile: .device),
              "Expected device policy to admit \(accepted)"
            )
        }
        for denied in [
          "ws://8.8.8.8:7447",
          "ws://device.example:7447",
          "wss://device.example:7447",
          "ws://0.0.0.0:7447",
          "ws://127.0.0.1:7447",
          "ws://169.254.1.1:7447",
          "ws://172.32.0.5:7447",
          "ws://100.64.0.5:7447",
          "ws://224.0.0.1:7447",
          "ws://[::]:7447",
          "ws://[::1]:7447",
          "ws://[fe80::1]:7447",
          "ws://[ff02::1]:7447",
          "ws://[2001:4860:4860::8888]:7447",
        ] {
            XCTAssertThrowsError(
              try TeraNetworkValidator.relays([denied], profile: .device),
              "Expected device policy to deny \(denied)"
            )
        }
        for denied in [
          "ws://public.example",
          "wss://localhost",
          "wss://10.0.0.1",
          "wss://user@example.com",
          "wss://relay.example?token=value",
        ] {
            XCTAssertThrowsError(
              try TeraNetworkValidator.relays([denied], profile: .publicNetwork),
              "Expected public policy to deny \(denied)"
            )
        }
    }

    func testLegacyRelayMigrationIsIdempotentAndCorruptionIsNotAbsence() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        let fileAccess = RadrootsAppleFileAccess(roots: fixture.roots)
        try fileAccess.write(
          .inline(
                Data(
                    """
                    {"format":"radroots_field_ios_relay_settings_v1","relays":["ws://127.0.0.1:7447"]}
                    """.utf8
                )
          ),
          to: RadrootsFileReference(
            scope: .data,
            relativePath: "settings/relay_settings.json"
          )
        )
        let store = TeraConfigurationStore(
          bootstrap: fixture.bootstrap,
          roots: fixture.roots
        )
        let first = try await store.load()
        let second = try await store.load()
        XCTAssertEqual(first, second)
        XCTAssertEqual(first.writableRelays, ["ws://127.0.0.1:7447"])

        try fileAccess.write(
          .inline(Data("not-json".utf8)),
          to: RadrootsFileReference(
            scope: .data,
            relativePath: "settings/radroots_configuration_v3.json"
          )
        )
        do {
            _ = try await store.load()
            XCTFail("Corrupt stored configuration must fail closed")
        } catch {
            XCTAssertEqual(error as? TeraConfigurationError, .corruptStoredConfiguration)
        }
    }

    func testV2ConfigurationMigratesOnceToExplicitV3BlossomAuthority() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        let fileAccess = RadrootsAppleFileAccess(roots: fixture.roots)
        try fileAccess.write(
          .inline(
                Data(
                    """
                    {"format":"radroots_ios_configuration_v2","profile":"simulator","writableRelays":["ws://127.0.0.1:7447"],"blossomOrigins":["http://127.0.0.1:3000","http://localhost:3001"]}
                    """.utf8
                )
          ),
          to: RadrootsFileReference(
            scope: .data,
            relativePath: "settings/radroots_configuration_v2.json"
          )
        )
        let store = TeraConfigurationStore(bootstrap: fixture.bootstrap, roots: fixture.roots)

        let first = try await store.load()
        let second = try await store.load()

        XCTAssertEqual(first, second)
        XCTAssertEqual(
          first.blossom,
          TeraBlossomEndpointConfiguration(
            hostKind: .simulator,
            endpointAuthority: .loopbackDevelopment,
            primaryOrigin: "http://127.0.0.1:3000",
            fallbackOrigins: ["http://localhost:3001"]
          )
        )
        let persisted = try configurationObject(fileAccess)
        XCTAssertEqual(persisted["format"] as? String, "radroots_ios_configuration_v3")
        let blossom = try XCTUnwrap(persisted["blossom"] as? [String: Any])
        XCTAssertEqual(blossom["hostKind"] as? String, "simulator")
        XCTAssertEqual(blossom["endpointAuthority"] as? String, "loopback_development")
        XCTAssertEqual(persisted["activationState"] as? String, "current")
        XCTAssertEqual(persisted["generation"] as? UInt64, 1)

        let fingerprint = String(repeating: "a", count: 64)
        try await store.confirmCanonicalBlossomConfiguration(
          canonicalBlossomConfiguration(
            primaryOrigin: "http://127.0.0.1:3000",
            fallbackOrigins: ["http://localhost:3001"],
            fingerprint: fingerprint
          ),
          expectedGeneration: first.generation
        )
        let confirmed = try configurationObject(fileAccess)
        XCTAssertEqual(confirmed["canonicalBlossomConfigFingerprint"] as? String, fingerprint)
        XCTAssertEqual(confirmed["activationState"] as? String, "current")
    }

    func testStoredProfileDriftRecoversFromCurrentBootstrap() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        let fileAccess = RadrootsAppleFileAccess(roots: fixture.roots)
        _ = try await TeraConfigurationStore(
          bootstrap: fixture.bootstrap,
          roots: fixture.roots
        ).load()
        let originalFingerprint = String(repeating: "b", count: 64)
        let originalStore = TeraConfigurationStore(
          bootstrap: fixture.bootstrap,
          roots: fixture.roots
        )
        try await originalStore.confirmCanonicalBlossomConfiguration(
          canonicalBlossomConfiguration(fingerprint: originalFingerprint),
          expectedGeneration: 1
        )
        let production = TeraConfigurationBootstrap(
          runtimeMode: "production",
          relayURLs: ["wss://write.example"],
          blossomOrigins: ["https://blossom.example"],
          keychainServicePrefix: fixture.bootstrap.keychainServicePrefix,
          bundleIdentifier: fixture.bootstrap.bundleIdentifier,
          appMetadata: fixture.bootstrap.appMetadata
        )
        let store = TeraConfigurationStore(bootstrap: production, roots: fixture.roots)

        let recovered = try await store.load()

        XCTAssertEqual(recovered.profile, .publicNetwork)
        XCTAssertEqual(recovered.activationState, .reconfigurationRequired)
        XCTAssertEqual(recovered.generation, 2)
        XCTAssertEqual(recovered.previousBlossomConfigFingerprint, originalFingerprint)
        XCTAssertEqual(recovered.writableRelays, ["wss://write.example"])
        XCTAssertEqual(
          recovered.blossom,
          TeraBlossomEndpointConfiguration(
            hostKind: .physicalDevice,
            endpointAuthority: .publicWebPKI,
            primaryOrigin: "https://blossom.example",
            fallbackOrigins: []
          )
        )
        let repeated = try await store.load()
        XCTAssertEqual(repeated, recovered)

        let persisted = try configurationObject(fileAccess)
        XCTAssertEqual(persisted["profile"] as? String, "public")
        XCTAssertEqual(persisted["activationState"] as? String, "reconfiguration_required")
        XCTAssertNil(persisted["canonicalBlossomConfigFingerprint"])
        XCTAssertEqual(
          persisted["previousBlossomConfigFingerprint"] as? String,
          originalFingerprint
        )
    }

    func testBootstrapInputDriftRequiresExplicitGenerationAwareActivation() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        let original = TeraConfigurationStore(
          bootstrap: fixture.bootstrap,
          roots: fixture.roots
        )
        let first = try await original.load()
        let originalFingerprint = String(repeating: "c", count: 64)
        try await original.confirmCanonicalBlossomConfiguration(
          canonicalBlossomConfiguration(fingerprint: originalFingerprint),
          expectedGeneration: first.generation
        )
        let changedBootstrap = TeraConfigurationBootstrap(
          runtimeMode: fixture.bootstrap.runtimeMode,
          relayURLs: ["ws://127.0.0.1:7448"],
          blossomOrigins: ["http://127.0.0.1:3001"],
          keychainServicePrefix: fixture.bootstrap.keychainServicePrefix,
          bundleIdentifier: fixture.bootstrap.bundleIdentifier,
          appMetadata: fixture.bootstrap.appMetadata
        )
        let changed = TeraConfigurationStore(
          bootstrap: changedBootstrap,
          roots: fixture.roots
        )

        let pending = try await changed.load()

        XCTAssertEqual(pending.activationState, .reconfigurationRequired)
        XCTAssertEqual(pending.generation, first.generation + 1)
        XCTAssertEqual(pending.previousBlossomConfigFingerprint, originalFingerprint)
        XCTAssertEqual(pending.writableRelays, ["ws://127.0.0.1:7448"])
        XCTAssertEqual(pending.blossom?.primaryOrigin, "http://127.0.0.1:3001")

        let replacementFingerprint = String(repeating: "d", count: 64)
        try await changed.confirmCanonicalBlossomConfiguration(
          canonicalBlossomConfiguration(
            primaryOrigin: "http://127.0.0.1:3001",
            fingerprint: replacementFingerprint
          ),
          expectedGeneration: pending.generation
        )
        let active = try await changed.load()
        XCTAssertEqual(active.activationState, .current)
        XCTAssertEqual(active.generation, pending.generation)
        XCTAssertNil(active.previousBlossomConfigFingerprint)
        let persisted = try configurationObject(
            RadrootsAppleFileAccess(roots: fixture.roots)
        )
        XCTAssertEqual(
          persisted["canonicalBlossomConfigFingerprint"] as? String,
          replacementFingerprint
        )
    }

    func testMaximumStoredGenerationFailsAsCorruptionWithoutWrapping() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        let fileAccess = RadrootsAppleFileAccess(roots: fixture.roots)
        _ = try await TeraConfigurationStore(
          bootstrap: fixture.bootstrap,
          roots: fixture.roots
        ).load()
        var persisted = try configurationObject(fileAccess)
        persisted["generation"] = NSNumber(value: UInt64.max)
        try fileAccess.write(
          .inline(JSONSerialization.data(withJSONObject: persisted, options: [.sortedKeys])),
          to: RadrootsFileReference(
            scope: .data,
            relativePath: "settings/radroots_configuration_v3.json"
          )
        )
        let changedBootstrap = TeraConfigurationBootstrap(
          runtimeMode: fixture.bootstrap.runtimeMode,
          relayURLs: ["ws://127.0.0.1:7448"],
          blossomOrigins: fixture.bootstrap.blossomOrigins,
          keychainServicePrefix: fixture.bootstrap.keychainServicePrefix,
          bundleIdentifier: fixture.bootstrap.bundleIdentifier,
          appMetadata: fixture.bootstrap.appMetadata
        )
        let changed = TeraConfigurationStore(
          bootstrap: changedBootstrap,
          roots: fixture.roots
        )

        do {
            _ = try await changed.load()
            XCTFail("Maximum persisted generation must fail closed")
        } catch {
            XCTAssertEqual(error as? TeraConfigurationError, .corruptStoredConfiguration)
        }
        XCTAssertEqual(try configurationObject(fileAccess)["generation"] as? UInt64, .max)
    }

    func testBootstrapActivationRetainsSelectedNetworkAndClearsPendingRollbackState() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        let original = TeraConfigurationStore(
          bootstrap: fixture.bootstrap,
          roots: fixture.roots
        )
        let first = try await original.load()
        try await original.confirmCanonicalBlossomConfiguration(
          canonicalBlossomConfiguration(fingerprint: String(repeating: "f", count: 64)),
          expectedGeneration: first.generation
        )
        let changedBootstrap = TeraConfigurationBootstrap(
          runtimeMode: fixture.bootstrap.runtimeMode,
          relayURLs: ["ws://127.0.0.1:7448"],
          blossomOrigins: ["http://127.0.0.1:3001"],
          keychainServicePrefix: fixture.bootstrap.keychainServicePrefix,
          bundleIdentifier: fixture.bootstrap.bundleIdentifier,
          appMetadata: fixture.bootstrap.appMetadata
        )
        let changed = TeraConfigurationStore(
          bootstrap: changedBootstrap,
          roots: fixture.roots
        )
        let pending = try await changed.load()

        try await changed.confirmBootstrapActivation(expectedGeneration: pending.generation)

        let active = try await changed.load()
        XCTAssertEqual(active.activationState, .current)
        XCTAssertEqual(active.generation, pending.generation)
        XCTAssertEqual(active.writableRelays, ["ws://127.0.0.1:7448"])
        XCTAssertEqual(active.blossom?.primaryOrigin, "http://127.0.0.1:3001")
        XCTAssertNil(active.previousBlossomConfigFingerprint)
        let persisted = try configurationObject(
            RadrootsAppleFileAccess(roots: fixture.roots)
        )
        XCTAssertNil(persisted["canonicalBlossomConfigFingerprint"])
        XCTAssertNil(persisted["previousBlossomConfigFingerprint"])
    }

    func testCanonicalPublicWebPkiRuntimeLabelPreservesStoredFormat() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        let bootstrap = TeraConfigurationBootstrap(
          runtimeMode: "production",
          relayURLs: [],
          blossomOrigins: ["https://media.example"],
          keychainServicePrefix: fixture.bootstrap.keychainServicePrefix,
          bundleIdentifier: fixture.bootstrap.bundleIdentifier,
          appMetadata: fixture.bootstrap.appMetadata
        )
        let store = TeraConfigurationStore(bootstrap: bootstrap, roots: fixture.roots)
        let selected = try await store.load()

        try await store.confirmCanonicalBlossomConfiguration(
          TeraBlossomConfigurationStatus(
            schemaVersion: 1,
            hostKind: "physical_device",
            endpointAuthority: "public_webpki",
            primaryOrigin: "https://media.example",
            fallbackOrigins: [],
            configFingerprint: String(repeating: "e", count: 64)
          ),
          expectedGeneration: selected.generation
        )

        let persisted = try configurationObject(
            RadrootsAppleFileAccess(roots: fixture.roots)
        )
        let blossom = try XCTUnwrap(persisted["blossom"] as? [String: Any])
        XCTAssertEqual(blossom["endpointAuthority"] as? String, "public_web_pki")
    }

    func testStoredBlossomAuthorityDriftRecoversWithinProfile() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        let fileAccess = RadrootsAppleFileAccess(roots: fixture.roots)
        try fileAccess.write(
          .inline(
                Data(
                    """
                    {"format":"radroots_ios_configuration_v3","profile":"simulator","writableRelays":["ws://127.0.0.1:7447"],"blossom":{"hostKind":"physical_device","endpointAuthority":"public_web_pki","primaryOrigin":"https://wrong.example","fallbackOrigins":[]}}
                    """.utf8
                )
          ),
          to: RadrootsFileReference(
            scope: .data,
            relativePath: "settings/radroots_configuration_v3.json"
          )
        )
        let store = TeraConfigurationStore(bootstrap: fixture.bootstrap, roots: fixture.roots)

        let recovered = try await store.load()

        XCTAssertEqual(
          recovered.blossom,
          TeraBlossomEndpointConfiguration(
            hostKind: .simulator,
            endpointAuthority: .loopbackDevelopment,
            primaryOrigin: "http://127.0.0.1:3000",
            fallbackOrigins: []
          )
        )
    }

    private func configurationObject(
        _ fileAccess: RadrootsAppleFileAccess
    ) throws -> [String: Any] {
        let source = try fileAccess.read(
          RadrootsFileReference(
            scope: .data,
            relativePath: "settings/radroots_configuration_v3.json"
          ),
          mode: .inline(maxBytes: TeraConfigurationStore.maximumStoredConfigurationBytes)
        )
        guard case let .inline(data) = source,
              let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            throw TeraConfigurationError.persistenceFailed
        }
        return object
    }

    private func canonicalBlossomConfiguration(
      primaryOrigin: String = "http://127.0.0.1:3000",
      fallbackOrigins: [String] = [],
      fingerprint: String
    ) -> TeraBlossomConfigurationStatus {
        TeraBlossomConfigurationStatus(
          schemaVersion: 1,
          hostKind: "simulator",
          endpointAuthority: "loopback_development",
          primaryOrigin: primaryOrigin,
          fallbackOrigins: fallbackOrigins,
          configFingerprint: fingerprint
        )
    }

    func testSourceGenerationAndVisualIdentitySurviveStoreRecreation() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        let firstStore = TeraConfigurationStore(
          bootstrap: fixture.bootstrap,
          roots: fixture.roots
        )
        let first = try await firstStore.sourceGeneration()
        let secondStore = TeraConfigurationStore(
          bootstrap: fixture.bootstrap,
          roots: fixture.roots
        )
        let second = try await secondStore.sourceGeneration()
        XCTAssertEqual(first, second)

        let key = String(repeating: "ab", count: 32)
        XCTAssertEqual(
          TeraStableVisualIdentity(publicKeyHex: key),
          TeraStableVisualIdentity(publicKeyHex: key)
        )
        XCTAssertNotEqual(
          TeraStableVisualIdentity(publicKeyHex: key).digestHex,
          TeraStableVisualIdentity(publicKeyHex: String(repeating: "cd", count: 32)).digestHex
        )
    }

    func testSourceGenerationRejectsInvalidInjectedClockWithoutPersistence() async throws {
        let fixture = try StateFixture()
        defer { fixture.remove() }
        for value in [TimeInterval.nan, -1, TimeInterval.greatestFiniteMagnitude, 0] {
            let store = TeraConfigurationStore(
              bootstrap: fixture.bootstrap,
              roots: fixture.roots,
              clock: TeraClock(now: { Date(timeIntervalSince1970: value) })
            )
            do {
                _ = try await store.sourceGeneration()
                XCTFail("Invalid clock value must fail closed")
            } catch {
                XCTAssertEqual(error as? TeraConfigurationError, .persistenceFailed)
            }
        }
        let fileAccess = RadrootsAppleFileAccess(roots: fixture.roots)
        XCTAssertThrowsError(
          try fileAccess.read(
            RadrootsFileReference(
              scope: .data,
              relativePath: "state/source_generation_v1.json"
            ),
            mode: .inline(maxBytes: TeraConfigurationStore.maximumStoredConfigurationBytes)
          )
        )
    }

    func testLegacyIdentityMigrationIsTransactionalAndIdempotent() async throws {
        let secureStore = InMemorySecureStore()
        let metadataStore = InMemoryIdentityMetadataStore()
        let servicePrefix = "org.radroots.tests.identity.\(UUID().uuidString.lowercased())"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: servicePrefix))
        defer {
            UserDefaults(suiteName: servicePrefix)?.removePersistentDomain(forName: servicePrefix)
        }
        let legacyKey = RadrootsSecureStoreKey(
          namespace: "nostr_identity",
          name: "selected_secret_hex"
        )
        try secureStore.put(
          Data(String(repeating: "01", count: 32).utf8),
          for: legacyKey,
          policy: .secureLocalSecret
        )
        let custody = try RadrootsIdentityCustody(
          configuration: RadrootsIdentityCustodyConfiguration(
            namespace: "radroots_identity_v1",
            secretPolicy: .secureLocalSecret
          ),
          secureStore: secureStore,
          metadataStore: metadataStore,
          userPresence: AllowingUserPresence()
        )
        let store = TeraIdentityStore(
          custody: custody,
          secureStore: secureStore,
          servicePrefix: servicePrefix,
          userDefaults: defaults
        )
        let first = try await store.loadAndMigrate()
        XCTAssertEqual(first.state, .recoveryRequired)
        XCTAssertTrue(try secureStore.contains(legacyKey))
        let migrated = try await store.recover()
        let second = try await store.loadAndMigrate()
        XCTAssertEqual(migrated.publicKeyHex, second.publicKeyHex)
        XCTAssertEqual(migrated.state, .unlocked)
        XCTAssertFalse(try secureStore.contains(legacyKey))
    }

    func testMalformedLegacyIdentityMetadataIsNotTreatedAsMissing() async throws {
        let secureStore = InMemorySecureStore()
        let servicePrefix = "org.radroots.tests.corrupt.\(UUID().uuidString.lowercased())"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: servicePrefix))
        defer {
            UserDefaults(suiteName: servicePrefix)?.removePersistentDomain(forName: servicePrefix)
        }
        defaults.set(
          Data("not-json".utf8),
          forKey: "field_ios.identity.public_metadata.\(servicePrefix)"
        )
        let custody = try RadrootsIdentityCustody(
          configuration: RadrootsIdentityCustodyConfiguration(
            namespace: "radroots_identity_v1",
            secretPolicy: .secureLocalSecret
          ),
          secureStore: secureStore,
          metadataStore: InMemoryIdentityMetadataStore(),
          userPresence: AllowingUserPresence()
        )
        let store = TeraIdentityStore(
          custody: custody,
          secureStore: secureStore,
          servicePrefix: servicePrefix,
          userDefaults: defaults
        )
        do {
            _ = try await store.loadAndMigrate()
            XCTFail("Malformed legacy metadata must be classified as corrupt")
        } catch {
            XCTAssertEqual(error as? TeraIdentityStoreError, .corruptLegacyMetadata)
        }
    }

    func testRuntimeSignerUsesCanonicalOperationIDInsteadOfSignerRequestDigest() async throws {
        let secureStore = InMemorySecureStore()
        let custody = try RadrootsIdentityCustody(
          configuration: RadrootsIdentityCustodyConfiguration(
            namespace: "radroots_identity_v1",
            secretPolicy: .secureLocalSecret
          ),
          secureStore: secureStore,
          metadataStore: InMemoryIdentityMetadataStore(),
          userPresence: AllowingUserPresence()
        )
        let servicePrefix = "org.radroots.tests.signer.\(UUID().uuidString.lowercased())"
        let store = TeraIdentityStore(
          custody: custody,
          secureStore: secureStore,
          servicePrefix: servicePrefix
        )
        let identity = try await store.create()
        let signer = try await store.signer(for: identity)
        let operationID = UUID().uuidString.lowercased()

        let outcome = try await signer.sign(
            TeraRuntimeSigningRequest(
              operationID: operationID,
              signerRequestID: String(repeating: "ab", count: 32),
              publicKeyHex: XCTUnwrap(identity.publicKeyHex),
              purpose: .blossomUpload,
              deadlineUnixMilliseconds: UInt64(Date().timeIntervalSince1970 * 1000) + 60000,
              digest: Data(repeating: 0xCD, count: 32)
            )
        )

        guard case let .signed(signatureHex) = outcome else {
            return XCTFail("The custody signer rejected the canonical runtime operation ID")
        }
        XCTAssertEqual(signatureHex.count, 128)
    }
}
