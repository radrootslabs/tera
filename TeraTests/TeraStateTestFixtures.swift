import Foundation
import RadrootsKit
@testable import TeraApp

struct StateFixture {
    let root: URL
    let roots: RadrootsAppleFileRoots
    let bootstrap: TeraConfigurationBootstrap

    init() throws {
        root = FileManager.default.temporaryDirectory
            .appendingPathComponent("radroots-state-tests-\(UUID().uuidString.lowercased())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        roots = try RadrootsAppleFileRoots(
          appIdentifier: "org.radroots.tests",
          dataRoot: root.appendingPathComponent("data"),
          cacheRoot: root.appendingPathComponent("cache"),
          temporaryRoot: root.appendingPathComponent("tmp")
        )
        bootstrap = TeraConfigurationBootstrap(
          runtimeMode: "localhost-dev",
          relayURLs: ["ws://127.0.0.1:8080"],
          blossomOrigins: ["http://127.0.0.1:3000"],
          keychainServicePrefix: "org.radroots.tests",
          bundleIdentifier: "org.radroots.tests",
          appMetadata: TeraRuntimeAppMetadata(
            bundleIdentifier: "org.radroots.tests",
            version: "0.1.0-alpha",
            buildNumber: "1",
            buildSHA: nil
          )
        )
    }

    func remove() {
        try? FileManager.default.removeItem(at: root)
    }
}

final class InMemorySecureStore: RadrootsSecureStore, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [RadrootsSecureStoreKey: Data] = [:]

    func put(
      _ value: Data,
      for key: RadrootsSecureStoreKey,
      policy _: RadrootsSecretAccessPolicy
    ) throws {
        lock.withLock { values[key] = value }
    }

    func contains(_ key: RadrootsSecureStoreKey) throws -> Bool {
        lock.withLock { values[key] != nil }
    }

    func get(_ key: RadrootsSecureStoreKey) throws -> Data? {
        lock.withLock { values[key] }
    }

    func delete(_ key: RadrootsSecureStoreKey) throws {
        lock.withLock { _ = values.removeValue(forKey: key) }
    }

    func deleteNamespace(_ namespace: String) throws {
        lock.withLock { values = values.filter { $0.key.namespace != namespace } }
    }
}

final class InMemoryIdentityMetadataStore: RadrootsIdentityMetadataStore, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [RadrootsIdentityMetadataSlot: Data] = [:]

    func data(for slot: RadrootsIdentityMetadataSlot) throws -> Data? {
        lock.withLock { values[slot] }
    }

    func put(_ data: Data, for slot: RadrootsIdentityMetadataSlot) throws {
        lock.withLock { values[slot] = data }
    }

    func delete(_ slot: RadrootsIdentityMetadataSlot) throws {
        lock.withLock { _ = values.removeValue(forKey: slot) }
    }
}

struct AllowingUserPresence: RadrootsUserPresence {
    func currentStatus() async throws -> RadrootsUserPresenceStatus {
        RadrootsUserPresenceStatus(
          support: .deviceCredential,
          biometryKind: .none,
          canEvaluateDeviceCredential: true,
          canEvaluateBiometrics: false
        )
    }

    func verify(_ request: RadrootsUserPresenceRequest) async throws -> RadrootsUserPresenceResult {
        RadrootsUserPresenceResult(policy: request.policy, verified: true)
    }
}
