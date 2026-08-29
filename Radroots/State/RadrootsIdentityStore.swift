import CryptoKit
import Foundation
import RadrootsKit

enum RadrootsAppIdentityState: String, Sendable, Equatable {
    case absent
    case locked
    case unlocked
    case protectedDataUnavailable
    case recoveryRequired
    case corrupt
}

struct RadrootsAppIdentity: Sendable, Equatable {
    let state: RadrootsAppIdentityState
    let identityHandle: String?
    let publicKeyHex: String?
    let label: String?
    let signerGeneration: String?
    let recoveryCode: String?
}

struct RadrootsStableVisualIdentity: Sendable, Equatable {
    let digestHex: String
    let paletteIndex: Int

    init(publicKeyHex: String, paletteCount: Int = 12) {
        let digest = SHA256.hash(data: Data("radroots.avatar.v1:\(publicKeyHex)".utf8))
        digestHex = digest.map { String(format: "%02x", $0) }.joined()
        paletteIndex = Int(Array(digest)[0]) % max(1, paletteCount)
    }
}

enum RadrootsIdentityStoreError: Error, Sendable, Equatable {
    case corruptLegacyMetadata
    case custody(String)
    case unavailable
}

extension RadrootsIdentityStoreError: LocalizedError {
    var errorDescription: String? {
        switch self {
        case .corruptLegacyMetadata:
            "Legacy identity metadata is corrupt and requires recovery."
        case .custody:
            "The local identity needs attention before Radroots can continue."
        case .unavailable:
            "The local identity is unavailable."
        }
    }
}

actor RadrootsIdentityStore {
    private struct LegacyMetadata: Codable {
        let selectedIdentityId: String
        let publicKeyHex: String
        let publicKeyNpub: String
        let label: String?
        let updatedAtUnix: UInt64
    }

    private let custody: RadrootsIdentityCustody
    private let secureStore: any RadrootsSecureStore
    private let servicePrefix: String
    private let userDefaults: UserDefaults

    init(
      custody: RadrootsIdentityCustody,
      secureStore: any RadrootsSecureStore,
      servicePrefix: String,
      userDefaults: UserDefaults = .standard
    ) {
        self.custody = custody
        self.secureStore = secureStore
        self.servicePrefix = servicePrefix
        self.userDefaults = userDefaults
    }

    @MainActor
    static func production(
      servicePrefix: String,
      protectedDataAvailable: @escaping @Sendable () -> Bool,
      qualification: RadrootsRemoteQualificationEnvironment? = nil
    ) throws -> RadrootsIdentityStore {
        let namespace = "radroots_identity_v1"
        let secureStore = RadrootsAppleKeychainSecureStore(servicePrefix: servicePrefix)
        let configuration: RadrootsIdentityCustodyConfiguration
        let userPresence: any RadrootsUserPresence
        #if DEBUG
            if qualification != nil {
                configuration = try RadrootsIdentityCustodyConfiguration(
                  namespace: namespace,
                  secretPolicy: .secureLocalSecret
                )
                userPresence = RadrootsRemoteQualificationUserPresence()
            } else {
                configuration = try RadrootsIdentityCustodyConfiguration(namespace: namespace)
                userPresence = RadrootsAppleUserPresence()
            }
        #else
            _ = qualification
            configuration = try RadrootsIdentityCustodyConfiguration(namespace: namespace)
            userPresence = RadrootsAppleUserPresence()
        #endif
        let custody = try RadrootsIdentityCustody(
          configuration: configuration,
          secureStore: secureStore,
          metadataStore: RadrootsAppleIdentityMetadataStore(
            namespace: namespace,
            keyPrefix: qualification?.identityMetadataKeyPrefix
                    ?? "org.radroots.ios.identity"
          ),
          userPresence: userPresence,
          protectedData: RadrootsProtectedDataProvider {
                protectedDataAvailable() ? .available : .unavailable
            }
        )
        return RadrootsIdentityStore(
          custody: custody,
          secureStore: secureStore,
          servicePrefix: servicePrefix
        )
    }

    func loadAndMigrate() async throws -> RadrootsAppIdentity {
        let initial = await custody.snapshot()
        guard initial.state == .absent else {
            return Self.appIdentity(initial)
        }

        let legacySecretKey = RadrootsSecureStoreKey(
          namespace: "nostr_identity",
          name: "selected_secret_hex"
        )
        let hasLegacySecret: Bool
        do {
            hasLegacySecret = try secureStore.contains(legacySecretKey)
        } catch {
            throw RadrootsIdentityStoreError.unavailable
        }
        let legacyMetadata = try loadLegacyMetadata()
        guard hasLegacySecret || legacyMetadata != nil else {
            return Self.appIdentity(initial)
        }
        guard hasLegacySecret else {
            throw RadrootsIdentityStoreError.corruptLegacyMetadata
        }
        return RadrootsAppIdentity(
          state: .recoveryRequired,
          identityHandle: nil,
          publicKeyHex: legacyMetadata?.publicKeyHex.lowercased(),
          label: legacyMetadata?.label,
          signerGeneration: nil,
          recoveryCode: "identity.legacy_migration_required"
        )
    }

    private func migrateLegacyIdentity() async throws -> RadrootsAppIdentity {
        let legacySecretKey = RadrootsSecureStoreKey(
          namespace: "nostr_identity",
          name: "selected_secret_hex"
        )
        let legacyMetadata = try loadLegacyMetadata()
        do {
            let migrated = try await custody.migrateLegacyIdentity(
              from: legacySecretKey,
              label: legacyMetadata?.label
            )
            if let metadata = legacyMetadata,
               metadata.publicKeyHex.lowercased() != migrated.identity?.publicKeyHex
            {
                throw RadrootsIdentityStoreError.corruptLegacyMetadata
            }
            deleteLegacyMetadata()
            return Self.appIdentity(migrated)
        } catch let error as RadrootsIdentityStoreError {
            throw error
        } catch let error as RadrootsIdentityCustodyError {
            throw RadrootsIdentityStoreError.custody(error.code)
        } catch {
            throw RadrootsIdentityStoreError.unavailable
        }
    }

    func create(label: String? = nil) async throws -> RadrootsAppIdentity {
        try await custody.createIdentity(label: label).appValue
    }

    func importIdentity(
      _ material: RadrootsIdentitySecretMaterial,
      label: String? = nil
    ) async throws -> RadrootsAppIdentity {
        try await custody.importIdentity(material, label: label).appValue
    }

    func snapshot() async -> RadrootsAppIdentity {
        await custody.snapshot().appValue
    }

    func unlock() async throws -> RadrootsAppIdentity {
        try await custody.unlockIdentity().appValue
    }

    func recover() async throws -> RadrootsAppIdentity {
        let snapshot = await custody.snapshot()
        if snapshot.state == .absent {
            let legacyKey = RadrootsSecureStoreKey(
              namespace: "nostr_identity",
              name: "selected_secret_hex"
            )
            if try secureStore.contains(legacyKey) {
                return try await migrateLegacyIdentity()
            }
        }
        return try await custody.recover().appValue
    }

    func lock() async {
        await custody.lockIdentity()
    }

    func signer(for identity: RadrootsAppIdentity) throws -> any RadrootsRuntimeSigner {
        guard identity.state == .unlocked,
              let signerHandle = identity.signerGeneration,
              let publicKeyHex = identity.publicKeyHex
        else {
            throw RadrootsIdentityStoreError.unavailable
        }
        return RadrootsAppleCustodySigner(
          custody: custody,
          signerHandle: signerHandle,
          publicKeyHex: publicKeyHex
        )
    }

    private func loadLegacyMetadata() throws -> LegacyMetadata? {
        let key = "field_ios.identity.public_metadata.\(servicePrefix)"
        guard let data = userDefaults.data(forKey: key) else { return nil }
        guard let value = try? JSONDecoder().decode(LegacyMetadata.self, from: data),
              value.publicKeyHex.count == 64,
              value.publicKeyHex.allSatisfy(\.isHexDigit)
        else {
            throw RadrootsIdentityStoreError.corruptLegacyMetadata
        }
        return value
    }

    private func deleteLegacyMetadata() {
        userDefaults.removeObject(
            forKey: "field_ios.identity.public_metadata.\(servicePrefix)"
        )
    }

    private static func appIdentity(_ snapshot: RadrootsIdentitySnapshot) -> RadrootsAppIdentity {
        RadrootsAppIdentity(
          state: snapshot.state.appValue,
          identityHandle: snapshot.identity?.identityHandle,
          publicKeyHex: snapshot.identity?.publicKeyHex,
          label: snapshot.identity?.label,
          signerGeneration: snapshot.signerHandle,
          recoveryCode: snapshot.recoveryCode
        )
    }
}

private final class RadrootsAppleCustodySigner: RadrootsRuntimeSigner, @unchecked Sendable {
    private let custody: RadrootsIdentityCustody
    private let signerHandle: String
    private let publicKeyHex: String

    init(custody: RadrootsIdentityCustody, signerHandle: String, publicKeyHex: String) {
        self.custody = custody
        self.signerHandle = signerHandle
        self.publicKeyHex = publicKeyHex
    }

    func availability() async -> RadrootsRuntimeSignerAvailability {
        let snapshot = await custody.snapshot()
        return switch snapshot.state {
        case .unlocked where snapshot.signerHandle == signerHandle:
            RadrootsRuntimeSignerAvailability.ready
        case .locked:
            RadrootsRuntimeSignerAvailability.locked
        default:
            RadrootsRuntimeSignerAvailability.unavailable
        }
    }

    func sign(_ request: RadrootsRuntimeSigningRequest) async -> RadrootsRuntimeSigningOutcome {
        guard request.publicKeyHex == publicKeyHex,
              request.digest.count == 32
        else {
            return .invalidated
        }
        do {
            let result = try await custody.sign(
                RadrootsOpaqueSignRequest(
                  operationID: request.operationID,
                  signerHandle: signerHandle,
                  publicKeyHex: request.publicKeyHex,
                  digest: request.digest,
                  purpose: request.purpose.appleValue,
                  deadlineUnixMilliseconds: request.deadlineUnixMilliseconds
                )
            )
            return .signed(signatureHex: result.signature.map { String(format: "%02x", $0) }.joined())
        } catch let error as RadrootsIdentityCustodyError {
            return switch error {
            case .identityLocked, .userPresenceRequired:
                RadrootsRuntimeSigningOutcome.locked
            case .cancelled:
                RadrootsRuntimeSigningOutcome.cancelled
            case .timedOut:
                RadrootsRuntimeSigningOutcome.timedOut
            case .staleSigner, .invalidSignRequest, .invalidSignature:
                RadrootsRuntimeSigningOutcome.invalidated
            case .protectedDataUnavailable, .storageUnavailable:
                RadrootsRuntimeSigningOutcome.unavailable
            default:
                RadrootsRuntimeSigningOutcome.failed
            }
        } catch {
            return .failed
        }
    }
}

private extension RadrootsIdentitySnapshot {
    var appValue: RadrootsAppIdentity {
        RadrootsAppIdentity(
          state: state.appValue,
          identityHandle: identity?.identityHandle,
          publicKeyHex: identity?.publicKeyHex,
          label: identity?.label,
          signerGeneration: signerHandle,
          recoveryCode: recoveryCode
        )
    }
}

private extension RadrootsIdentityState {
    var appValue: RadrootsAppIdentityState {
        switch self {
        case .absent: .absent
        case .locked: .locked
        case .unlocked: .unlocked
        case .protectedDataUnavailable: .protectedDataUnavailable
        case .recoveryRequired: .recoveryRequired
        case .corrupt: .corrupt
        }
    }
}

private extension RadrootsRuntimeSigningPurpose {
    var appleValue: RadrootsOpaqueSignPurpose {
        switch self {
        case .nostrEvent: .nostrEvent
        case .blossomUpload: .blossomUpload
        }
    }
}
