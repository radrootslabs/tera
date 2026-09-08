import CryptoKit
import Foundation
import RadrootsKit

enum TeraAppIdentityState: String, Sendable, Equatable {
    case absent
    case locked
    case unlocked
    case protectedDataUnavailable
    case recoveryRequired
    case corrupt
}

struct TeraAppIdentity: Sendable, Equatable {
    let state: TeraAppIdentityState
    let identityHandle: String?
    let publicKeyHex: String?
    let label: String?
    let signerGeneration: String?
    let recoveryCode: String?
}

struct TeraStableVisualIdentity: Sendable, Equatable {
    let digestHex: String
    let paletteIndex: Int

    init(publicKeyHex: String, paletteCount: Int = 12) {
        let digest = SHA256.hash(data: Data("radroots.avatar.v1:\(publicKeyHex)".utf8))
        digestHex = digest.map { String(format: "%02x", $0) }.joined()
        paletteIndex = Int(Array(digest)[0]) % max(1, paletteCount)
    }
}

enum TeraIdentityStoreError: Error, Sendable, Equatable {
    case corruptLegacyMetadata
    case custody(String)
    case unavailable
}

extension TeraIdentityStoreError: LocalizedError {
    var errorDescription: String? {
        switch self {
        case .corruptLegacyMetadata:
            "Legacy identity metadata is corrupt and requires recovery."
        case .custody:
            "The local identity needs attention before Tera can continue."
        case .unavailable:
            "The local identity is unavailable."
        }
    }
}

actor TeraIdentityStore {
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
      qualification: TeraRemoteQualificationEnvironment? = nil
    ) throws -> TeraIdentityStore {
        let namespace = "radroots_identity_v1"
        let secureStore = RadrootsAppleKeychainSecureStore(servicePrefix: servicePrefix)
        let configuration: RadrootsIdentityCustodyConfiguration
        let userPresence: any RadrootsUserPresence
        #if DEBUG
            if let qualification, qualification.automatesIdentity {
                configuration = try RadrootsIdentityCustodyConfiguration(
                  namespace: namespace,
                  secretPolicy: .secureLocalSecret
                )
                userPresence = try TeraRemoteQualificationUserPresence(
                  mode: qualification.networkMode
                )
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
        return TeraIdentityStore(
          custody: custody,
          secureStore: secureStore,
          servicePrefix: servicePrefix
        )
    }

    func loadAndMigrate() async throws -> TeraAppIdentity {
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
            throw TeraIdentityStoreError.unavailable
        }
        let legacyMetadata = try loadLegacyMetadata()
        guard hasLegacySecret || legacyMetadata != nil else {
            return Self.appIdentity(initial)
        }
        guard hasLegacySecret else {
            throw TeraIdentityStoreError.corruptLegacyMetadata
        }
        return TeraAppIdentity(
          state: .recoveryRequired,
          identityHandle: nil,
          publicKeyHex: legacyMetadata?.publicKeyHex.lowercased(),
          label: legacyMetadata?.label,
          signerGeneration: nil,
          recoveryCode: "identity.legacy_migration_required"
        )
    }

    private func migrateLegacyIdentity() async throws -> TeraAppIdentity {
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
                throw TeraIdentityStoreError.corruptLegacyMetadata
            }
            deleteLegacyMetadata()
            return Self.appIdentity(migrated)
        } catch let error as TeraIdentityStoreError {
            throw error
        } catch let error as RadrootsIdentityCustodyError {
            throw TeraIdentityStoreError.custody(error.code)
        } catch {
            throw TeraIdentityStoreError.unavailable
        }
    }

    func create(label: String? = nil) async throws -> TeraAppIdentity {
        try await custody.createIdentity(label: label).appValue
    }

    func importIdentity(
      _ material: RadrootsIdentitySecretMaterial,
      label: String? = nil
    ) async throws -> TeraAppIdentity {
        try await custody.importIdentity(material, label: label).appValue
    }

    func snapshot() async -> TeraAppIdentity {
        await custody.snapshot().appValue
    }

    func unlock() async throws -> TeraAppIdentity {
        try await custody.unlockIdentity().appValue
    }

    func recover() async throws -> TeraAppIdentity {
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

    func signer(for identity: TeraAppIdentity) throws -> any TeraRuntimeSigner {
        guard identity.state == .unlocked,
              let signerHandle = identity.signerGeneration,
              let publicKeyHex = identity.publicKeyHex
        else {
            throw TeraIdentityStoreError.unavailable
        }
        return TeraAppleCustodySigner(
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
            throw TeraIdentityStoreError.corruptLegacyMetadata
        }
        return value
    }

    private func deleteLegacyMetadata() {
        userDefaults.removeObject(
            forKey: "field_ios.identity.public_metadata.\(servicePrefix)"
        )
    }

    private static func appIdentity(_ snapshot: RadrootsIdentitySnapshot) -> TeraAppIdentity {
        TeraAppIdentity(
          state: snapshot.state.appValue,
          identityHandle: snapshot.identity?.identityHandle,
          publicKeyHex: snapshot.identity?.publicKeyHex,
          label: snapshot.identity?.label,
          signerGeneration: snapshot.signerHandle,
          recoveryCode: snapshot.recoveryCode
        )
    }
}

private final class TeraAppleCustodySigner: TeraRuntimeSigner, @unchecked Sendable {
    private let custody: RadrootsIdentityCustody
    private let signerHandle: String
    private let publicKeyHex: String

    init(custody: RadrootsIdentityCustody, signerHandle: String, publicKeyHex: String) {
        self.custody = custody
        self.signerHandle = signerHandle
        self.publicKeyHex = publicKeyHex
    }

    func availability() async -> TeraRuntimeSignerAvailability {
        let snapshot = await custody.snapshot()
        return switch snapshot.state {
        case .unlocked where snapshot.signerHandle == signerHandle:
            TeraRuntimeSignerAvailability.ready
        case .locked:
            TeraRuntimeSignerAvailability.locked
        default:
            TeraRuntimeSignerAvailability.unavailable
        }
    }

    func sign(_ request: TeraRuntimeSigningRequest) async -> TeraRuntimeSigningOutcome {
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
                TeraRuntimeSigningOutcome.locked
            case .cancelled:
                TeraRuntimeSigningOutcome.cancelled
            case .timedOut:
                TeraRuntimeSigningOutcome.timedOut
            case .staleSigner, .invalidSignRequest, .invalidSignature:
                TeraRuntimeSigningOutcome.invalidated
            case .protectedDataUnavailable, .storageUnavailable:
                TeraRuntimeSigningOutcome.unavailable
            default:
                TeraRuntimeSigningOutcome.failed
            }
        } catch {
            return .failed
        }
    }
}

private extension RadrootsIdentitySnapshot {
    var appValue: TeraAppIdentity {
        TeraAppIdentity(
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
    var appValue: TeraAppIdentityState {
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

private extension TeraRuntimeSigningPurpose {
    var appleValue: RadrootsOpaqueSignPurpose {
        switch self {
        case .nostrEvent: .nostrEvent
        case .blossomUpload: .blossomUpload
        }
    }
}
