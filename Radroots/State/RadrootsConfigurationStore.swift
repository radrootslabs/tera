import Darwin
import CryptoKit
import Foundation
import RadrootsKit
import Security

enum RadrootsAppNetworkProfile: String, Codable, Sendable, Equatable {
    case publicNetwork = "public"
    case simulator
    case device

    var runtimeValue: RadrootsRuntimeNetworkProfile {
        switch self {
        case .publicNetwork: .publicNetwork
        case .simulator: .simulator
        case .device: .device
        }
    }
}

struct RadrootsAppConfiguration: Sendable, Equatable {
    let profile: RadrootsAppNetworkProfile
    let writableRelays: [String]
    let blossom: RadrootsBlossomEndpointConfiguration?
    let keychainServicePrefix: String
    let bundleIdentifier: String
    let appMetadata: RadrootsRuntimeAppMetadata
    let generation: UInt64
    let activationState: RadrootsConfigurationActivationState
    let previousBlossomConfigFingerprint: String?
}

enum RadrootsConfigurationActivationState: String, Codable, Sendable, Equatable {
    case current
    case reconfigurationRequired = "reconfiguration_required"
}

struct RadrootsConfigurationBootstrap: Sendable, Equatable {
    let runtimeMode: String
    let relayURLs: [String]
    let blossomOrigins: [String]
    let keychainServicePrefix: String
    let bundleIdentifier: String
    let appMetadata: RadrootsRuntimeAppMetadata
}

struct RadrootsSourceGeneration: Codable, Sendable, Equatable {
    let schemaVersion: UInt16
    let generationHex: String
    let createdAtUnixMilliseconds: UInt64
}

enum RadrootsConfigurationError: Error, Sendable, Equatable {
    case missing(String)
    case invalid(String)
    case corruptStoredConfiguration
    case corruptSourceGeneration
    case persistenceFailed
}

extension RadrootsConfigurationError: LocalizedError {
    var errorDescription: String? {
        switch self {
        case .missing:
            "Required Radroots configuration is missing."
        case .invalid:
            "Radroots network configuration is invalid."
        case .corruptStoredConfiguration:
            "Stored Radroots settings are corrupt and require recovery."
        case .corruptSourceGeneration:
            "Stored Radroots local-state identity is corrupt and requires recovery."
        case .persistenceFailed:
            "Radroots could not persist its local configuration."
        }
    }
}

actor RadrootsConfigurationStore {
    private struct StoredConfigurationV3: Codable, Equatable {
        static let format = "radroots_ios_configuration_v3"

        let format: String
        let profile: RadrootsAppNetworkProfile
        let writableRelays: [String]
        let blossom: RadrootsBlossomEndpointConfiguration?
        let generation: UInt64?
        let activationState: RadrootsConfigurationActivationState?
        let bootstrapFingerprint: String?
        let canonicalBlossomConfigFingerprint: String?
        let previousBlossomConfigFingerprint: String?
    }

    private struct BootstrapNetworkIdentity: Codable {
        let profile: RadrootsAppNetworkProfile
        let relayURLs: [String]
        let blossomOrigins: [String]
    }

    private struct StoredConfigurationV2: Codable {
        static let format = "radroots_ios_configuration_v2"

        let format: String
        let profile: RadrootsAppNetworkProfile
        let writableRelays: [String]
        let blossomOrigins: [String]
    }

    private struct LegacyRelaySettings: Codable {
        static let format = "radroots_field_ios_relay_settings_v1"

        let format: String
        let relays: [String]
    }

    private static let configurationFile = RadrootsFileReference(
        scope: .data,
        relativePath: "settings/radroots_configuration_v3.json"
    )
    private static let v2ConfigurationFile = RadrootsFileReference(
        scope: .data,
        relativePath: "settings/radroots_configuration_v2.json"
    )
    private static let legacyRelayFile = RadrootsFileReference(
        scope: .data,
        relativePath: "settings/relay_settings.json"
    )
    private static let sourceGenerationFile = RadrootsFileReference(
        scope: .data,
        relativePath: "state/source_generation_v1.json"
    )

    private let bootstrap: RadrootsConfigurationBootstrap
    private let fileAccess: RadrootsAppleFileAccess

    init(bootstrap: RadrootsConfigurationBootstrap, roots: RadrootsAppleFileRoots) {
        self.bootstrap = bootstrap
        fileAccess = RadrootsAppleFileAccess(roots: roots)
    }

    func load() throws -> RadrootsAppConfiguration {
        let profile = try Self.profile(for: bootstrap.runtimeMode)
        let bootstrapFingerprint = try Self.bootstrapFingerprint(bootstrap, profile: profile)
        let selected: StoredConfigurationV3
        if let stored = try readStoredConfiguration() {
            guard stored.format == StoredConfigurationV3.format else {
                throw RadrootsConfigurationError.corruptStoredConfiguration
            }
            let upgraded = try Self.upgradeStoredConfiguration(
                stored,
                bootstrapFingerprint: bootstrapFingerprint
            )
            if upgraded.profile == profile,
               Self.blossomMatchesProfile(upgraded.blossom, profile: profile),
               upgraded.bootstrapFingerprint == bootstrapFingerprint
            {
                selected = upgraded
                if selected != stored {
                    try persist(selected)
                }
            } else {
                selected = Self.bootstrapConfiguration(
                    bootstrap,
                    profile: profile,
                    generation: (upgraded.generation ?? 0) + 1,
                    activationState: .reconfigurationRequired,
                    bootstrapFingerprint: bootstrapFingerprint,
                    previousBlossomConfigFingerprint: upgraded.canonicalBlossomConfigFingerprint
                        ?? upgraded.previousBlossomConfigFingerprint
                )
                try persist(selected)
            }
        } else if let stored = try readV2Configuration() {
            guard stored.format == StoredConfigurationV2.format else {
                throw RadrootsConfigurationError.corruptStoredConfiguration
            }
            selected = if stored.profile == profile {
                StoredConfigurationV3(
                    format: StoredConfigurationV3.format,
                    profile: profile,
                    writableRelays: stored.writableRelays,
                    blossom: Self.blossomConfiguration(
                        origins: stored.blossomOrigins,
                        profile: profile
                    ),
                    generation: 1,
                    activationState: .current,
                    bootstrapFingerprint: bootstrapFingerprint,
                    canonicalBlossomConfigFingerprint: nil,
                    previousBlossomConfigFingerprint: nil
                )
            } else {
                Self.bootstrapConfiguration(
                    bootstrap,
                    profile: profile,
                    generation: 1,
                    activationState: .reconfigurationRequired,
                    bootstrapFingerprint: bootstrapFingerprint
                )
            }
            try persist(selected)
        } else if let legacy = try readLegacyConfiguration() {
            guard legacy.format == LegacyRelaySettings.format else {
                throw RadrootsConfigurationError.corruptStoredConfiguration
            }
            selected = StoredConfigurationV3(
                format: StoredConfigurationV3.format,
                profile: profile,
                writableRelays: legacy.relays,
                blossom: Self.blossomConfiguration(
                    origins: bootstrap.blossomOrigins,
                    profile: profile
                ),
                generation: 1,
                activationState: .current,
                bootstrapFingerprint: bootstrapFingerprint,
                canonicalBlossomConfigFingerprint: nil,
                previousBlossomConfigFingerprint: nil
            )
            try persist(selected)
        } else {
            selected = Self.bootstrapConfiguration(
                bootstrap,
                profile: profile,
                generation: 1,
                activationState: .current,
                bootstrapFingerprint: bootstrapFingerprint
            )
            try persist(selected)
        }

        let relays = try RadrootsNetworkValidator.relays(
            selected.writableRelays,
            profile: profile
        )
        guard !bootstrap.keychainServicePrefix.isEmpty,
              !bootstrap.bundleIdentifier.isEmpty
        else {
            throw RadrootsConfigurationError.missing("identity")
        }
        return RadrootsAppConfiguration(
            profile: profile,
            writableRelays: relays,
            blossom: selected.blossom,
            keychainServicePrefix: bootstrap.keychainServicePrefix,
            bundleIdentifier: bootstrap.bundleIdentifier,
            appMetadata: bootstrap.appMetadata,
            generation: selected.generation ?? 1,
            activationState: selected.activationState ?? .current,
            previousBlossomConfigFingerprint: selected.previousBlossomConfigFingerprint
        )
    }

    func confirmCanonicalBlossomConfiguration(
        _ canonical: RadrootsBlossomConfigurationStatus?,
        expectedGeneration: UInt64
    ) throws {
        guard let stored = try readStoredConfiguration(),
              stored.format == StoredConfigurationV3.format,
              stored.generation == expectedGeneration,
              let bootstrapFingerprint = stored.bootstrapFingerprint
        else {
            throw RadrootsConfigurationError.persistenceFailed
        }
        let canonicalBlossom: RadrootsBlossomEndpointConfiguration?
        let canonicalFingerprint: String?
        switch (stored.blossom, canonical) {
        case (nil, nil):
            canonicalBlossom = nil
            canonicalFingerprint = nil
        case let (.some(selected), .some(canonical)):
            guard let hostKind = RadrootsBlossomHostKind(rawValue: canonical.hostKind),
                  let endpointAuthority = RadrootsBlossomEndpointAuthority(
                      runtimeValue: canonical.endpointAuthority
                  ),
                  hostKind == selected.hostKind,
                  endpointAuthority == selected.endpointAuthority,
                  Self.isFingerprint(canonical.configFingerprint)
            else {
                throw RadrootsConfigurationError.invalid("canonical_blossom_configuration")
            }
            canonicalBlossom = RadrootsBlossomEndpointConfiguration(
                hostKind: hostKind,
                endpointAuthority: endpointAuthority,
                primaryOrigin: canonical.primaryOrigin,
                fallbackOrigins: canonical.fallbackOrigins
            )
            canonicalFingerprint = canonical.configFingerprint
        default:
            throw RadrootsConfigurationError.invalid("canonical_blossom_configuration")
        }
        try persist(
            StoredConfigurationV3(
                format: StoredConfigurationV3.format,
                profile: stored.profile,
                writableRelays: stored.writableRelays,
                blossom: canonicalBlossom,
                generation: expectedGeneration,
                activationState: .current,
                bootstrapFingerprint: bootstrapFingerprint,
                canonicalBlossomConfigFingerprint: canonicalFingerprint,
                previousBlossomConfigFingerprint: nil
            )
        )
    }

    func confirmBootstrapActivation(expectedGeneration: UInt64) throws {
        guard let stored = try readStoredConfiguration(),
              stored.format == StoredConfigurationV3.format,
              stored.generation == expectedGeneration,
              let bootstrapFingerprint = stored.bootstrapFingerprint
        else {
            throw RadrootsConfigurationError.persistenceFailed
        }
        try persist(
            StoredConfigurationV3(
                format: stored.format,
                profile: stored.profile,
                writableRelays: stored.writableRelays,
                blossom: stored.blossom,
                generation: expectedGeneration,
                activationState: .current,
                bootstrapFingerprint: bootstrapFingerprint,
                canonicalBlossomConfigFingerprint: nil,
                previousBlossomConfigFingerprint: nil
            )
        )
    }

    func sourceGeneration() throws -> RadrootsSourceGeneration {
        if let data = try read(Self.sourceGenerationFile) {
            guard let value = try? JSONDecoder().decode(RadrootsSourceGeneration.self, from: data),
                  value.schemaVersion == 1,
                  value.generationHex.count == 64,
                  value.generationHex.allSatisfy(\.isHexDigit),
                  value.generationHex == value.generationHex.lowercased(),
                  value.createdAtUnixMilliseconds > 0
            else {
                throw RadrootsConfigurationError.corruptSourceGeneration
            }
            return value
        }
        var bytes = [UInt8](repeating: 0, count: 32)
        guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
            throw RadrootsConfigurationError.persistenceFailed
        }
        let value = RadrootsSourceGeneration(
            schemaVersion: 1,
            generationHex: bytes.map { String(format: "%02x", $0) }.joined(),
            createdAtUnixMilliseconds: max(1, UInt64(Date().timeIntervalSince1970 * 1000))
        )
        do {
            let data = try JSONEncoder.radroots.encode(value)
            try fileAccess.write(.inline(data), to: Self.sourceGenerationFile)
            return value
        } catch let error as RadrootsConfigurationError {
            throw error
        } catch {
            throw RadrootsConfigurationError.persistenceFailed
        }
    }

    private func readStoredConfiguration() throws -> StoredConfigurationV3? {
        guard let data = try read(Self.configurationFile) else { return nil }
        guard let stored = try? JSONDecoder().decode(StoredConfigurationV3.self, from: data) else {
            throw RadrootsConfigurationError.corruptStoredConfiguration
        }
        return stored
    }

    private func readV2Configuration() throws -> StoredConfigurationV2? {
        guard let data = try read(Self.v2ConfigurationFile) else { return nil }
        guard let stored = try? JSONDecoder().decode(StoredConfigurationV2.self, from: data) else {
            throw RadrootsConfigurationError.corruptStoredConfiguration
        }
        return stored
    }

    private func readLegacyConfiguration() throws -> LegacyRelaySettings? {
        guard let data = try read(Self.legacyRelayFile) else { return nil }
        guard let legacy = try? JSONDecoder().decode(LegacyRelaySettings.self, from: data) else {
            throw RadrootsConfigurationError.corruptStoredConfiguration
        }
        return legacy
    }

    private func read(_ file: RadrootsFileReference) throws -> Data? {
        do {
            guard case let .inline(data) = try fileAccess.read(file, mode: .inline) else {
                throw RadrootsConfigurationError.persistenceFailed
            }
            return data
        } catch RadrootsAppleFileError.notFound {
            return nil
        } catch let error as RadrootsConfigurationError {
            throw error
        } catch {
            throw RadrootsConfigurationError.persistenceFailed
        }
    }

    private func persist(_ configuration: StoredConfigurationV3) throws {
        do {
            let data = try JSONEncoder.radroots.encode(configuration)
            try fileAccess.write(.inline(data), to: Self.configurationFile)
        } catch {
            throw RadrootsConfigurationError.persistenceFailed
        }
    }

    private static func profile(for runtimeMode: String) throws -> RadrootsAppNetworkProfile {
        switch runtimeMode.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "production": .publicNetwork
        case "localhost-dev", "simulator": .simulator
        case "device-development", "device": .device
        default: throw RadrootsConfigurationError.invalid("runtime_mode")
        }
    }

    private static func bootstrapConfiguration(
        _ bootstrap: RadrootsConfigurationBootstrap,
        profile: RadrootsAppNetworkProfile,
        generation: UInt64,
        activationState: RadrootsConfigurationActivationState,
        bootstrapFingerprint: String,
        previousBlossomConfigFingerprint: String? = nil
    ) -> StoredConfigurationV3 {
        StoredConfigurationV3(
            format: StoredConfigurationV3.format,
            profile: profile,
            writableRelays: bootstrap.relayURLs,
            blossom: blossomConfiguration(origins: bootstrap.blossomOrigins, profile: profile),
            generation: generation,
            activationState: activationState,
            bootstrapFingerprint: bootstrapFingerprint,
            canonicalBlossomConfigFingerprint: nil,
            previousBlossomConfigFingerprint: previousBlossomConfigFingerprint
        )
    }

    private static func upgradeStoredConfiguration(
        _ stored: StoredConfigurationV3,
        bootstrapFingerprint: String
    ) throws -> StoredConfigurationV3 {
        if let generation = stored.generation,
           let activationState = stored.activationState,
           let persistedBootstrapFingerprint = stored.bootstrapFingerprint
        {
            guard generation > 0,
                  isFingerprint(persistedBootstrapFingerprint),
                  stored.canonicalBlossomConfigFingerprint.map(isFingerprint) ?? true,
                  stored.previousBlossomConfigFingerprint.map(isFingerprint) ?? true
            else {
                throw RadrootsConfigurationError.corruptStoredConfiguration
            }
            return StoredConfigurationV3(
                format: stored.format,
                profile: stored.profile,
                writableRelays: stored.writableRelays,
                blossom: stored.blossom,
                generation: generation,
                activationState: activationState,
                bootstrapFingerprint: persistedBootstrapFingerprint,
                canonicalBlossomConfigFingerprint: stored.canonicalBlossomConfigFingerprint,
                previousBlossomConfigFingerprint: stored.previousBlossomConfigFingerprint
            )
        }
        guard stored.generation == nil,
              stored.activationState == nil,
              stored.bootstrapFingerprint == nil,
              stored.canonicalBlossomConfigFingerprint == nil,
              stored.previousBlossomConfigFingerprint == nil
        else {
            throw RadrootsConfigurationError.corruptStoredConfiguration
        }
        return StoredConfigurationV3(
            format: stored.format,
            profile: stored.profile,
            writableRelays: stored.writableRelays,
            blossom: stored.blossom,
            generation: 1,
            activationState: .current,
            bootstrapFingerprint: bootstrapFingerprint,
            canonicalBlossomConfigFingerprint: nil,
            previousBlossomConfigFingerprint: nil
        )
    }

    private static func bootstrapFingerprint(
        _ bootstrap: RadrootsConfigurationBootstrap,
        profile: RadrootsAppNetworkProfile
    ) throws -> String {
        do {
            let data = try JSONEncoder.radroots.encode(
                BootstrapNetworkIdentity(
                    profile: profile,
                    relayURLs: bootstrap.relayURLs,
                    blossomOrigins: bootstrap.blossomOrigins
                )
            )
            return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        } catch {
            throw RadrootsConfigurationError.invalid("bootstrap_network_identity")
        }
    }

    private static func isFingerprint(_ value: String) -> Bool {
        value.count == 64
            && value == value.lowercased()
            && value.allSatisfy(\.isHexDigit)
    }

    private static func blossomConfiguration(
        origins: [String],
        profile: RadrootsAppNetworkProfile
    ) -> RadrootsBlossomEndpointConfiguration? {
        guard let primary = origins.first else { return nil }
        let identity = blossomIdentity(profile: profile)
        return RadrootsBlossomEndpointConfiguration(
            hostKind: identity.hostKind,
            endpointAuthority: identity.endpointAuthority,
            primaryOrigin: primary,
            fallbackOrigins: Array(origins.dropFirst())
        )
    }

    private static func blossomMatchesProfile(
        _ configuration: RadrootsBlossomEndpointConfiguration?,
        profile: RadrootsAppNetworkProfile
    ) -> Bool {
        guard let configuration else { return true }
        let identity = blossomIdentity(profile: profile)
        return configuration.hostKind == identity.hostKind
            && configuration.endpointAuthority == identity.endpointAuthority
    }

    private static func blossomIdentity(
        profile: RadrootsAppNetworkProfile
    ) -> (
        hostKind: RadrootsBlossomHostKind,
        endpointAuthority: RadrootsBlossomEndpointAuthority
    ) {
        switch profile {
        case .publicNetwork:
            (.physicalDevice, .publicWebPKI)
        case .simulator:
            (.simulator, .loopbackDevelopment)
        case .device:
            (.physicalDevice, .privateNetworkDevelopment)
        }
    }
}

enum RadrootsNetworkValidator {
    static let canonicalRelay = "wss://radroots.org"

    static func relays(
        _ values: [String],
        profile: RadrootsAppNetworkProfile
    ) throws -> [String] {
        var output: [String] = []
        var seen = Set<String>()
        for raw in values {
            let canonical = try relay(raw, profile: profile)
            if profile != .simulator, canonical == canonicalRelay {
                continue
            }
            guard seen.insert(canonical).inserted else {
                throw RadrootsConfigurationError.invalid("duplicate_relay")
            }
            output.append(canonical)
        }
        if profile != .publicNetwork, output.isEmpty {
            throw RadrootsConfigurationError.invalid("empty_relay_set")
        }
        let totalCount = output.count + (profile == .simulator ? 0 : 1)
        guard totalCount <= 64 else {
            throw RadrootsConfigurationError.invalid("too_many_relays")
        }
        return output
    }

    private static func relay(
        _ raw: String,
        profile: RadrootsAppNetworkProfile
    ) throws -> String {
        guard raw == raw.trimmingCharacters(in: .whitespacesAndNewlines),
              raw.utf8.count <= 2048,
              !raw.contains(where: { $0.isASCII && ($0.isWhitespace || $0.asciiValue ?? 32 < 32) }),
              !raw.contains("?"),
              !raw.contains("#"),
              !raw.contains("\\"),
              let components = URLComponents(string: raw),
              components.user == nil,
              components.password == nil,
              let scheme = components.scheme?.lowercased(),
              let parsedHost = components.host?.lowercased(),
              components.port != 0,
              components.path.isEmpty || components.path == "/"
        else {
            throw RadrootsConfigurationError.invalid("relay_url")
        }
        let host = parsedHost.hasPrefix("[") && parsedHost.hasSuffix("]")
            ? String(parsedHost.dropFirst().dropLast())
            : parsedHost
        let schemeAllowed = scheme == "wss"
            || (scheme == "ws" && (profile == .simulator || profile == .device))
        guard schemeAllowed, hostAllowed(host, profile: profile) else {
            throw RadrootsConfigurationError.invalid("relay_policy")
        }
        var canonical = "\(scheme)://\(hostForURL(host))"
        if let port = components.port,
           !((scheme == "wss" && port == 443) || (scheme == "ws" && port == 80))
        {
            canonical += ":\(port)"
        }
        return canonical
    }

    private static func hostForURL(_ host: String) -> String {
        host.contains(":") ? "[\(host)]" : host
    }

    private static func hostAllowed(
        _ host: String,
        profile: RadrootsAppNetworkProfile
    ) -> Bool {
        if profile == .simulator {
            return host == "localhost" || host == "127.0.0.1" || host == "::1"
        }
        if host == "localhost" || host.hasSuffix(".localhost") {
            return false
        }
        if let ipv4 = IPv4Address(host) {
            return profile == .publicNetwork ? ipv4.isPublic
                : profile == .device ? ipv4.isPrivateLAN : ipv4.isTrustedDevice
        }
        if let ipv6 = IPv6Address(host) {
            return profile == .publicNetwork ? ipv6.isPublic
                : profile == .device ? ipv6.isPrivateLAN : ipv6.isTrustedDevice
        }
        if profile == .device {
            return false
        }
        let normalized = host.trimmingCharacters(in: CharacterSet(charactersIn: "."))
        return normalized.contains(".")
            && !normalized.hasSuffix(".local")
            && !normalized.hasSuffix(".home.arpa")
    }
}

private struct IPv4Address {
    private var address = in_addr()

    init?(_ value: String) {
        guard inet_pton(AF_INET, value, &address) == 1 else { return nil }
    }

    var isTrustedDevice: Bool {
        var address = address
        let octets = withUnsafeBytes(of: &address.s_addr) { Array($0) }
        return octets[0] != 0 && octets[0] != 127 && !(224 ... 239).contains(octets[0])
            && octets != [255, 255, 255, 255]
    }

    var isPublic: Bool {
        var address = address
        let octets = withUnsafeBytes(of: &address.s_addr) { Array($0) }
        guard isTrustedDevice else { return false }
        return !(octets[0] == 10
            || octets[0] == 169 && octets[1] == 254
            || octets[0] == 172 && (16 ... 31).contains(octets[1])
            || octets[0] == 192 && octets[1] == 168
            || octets[0] == 100 && (64 ... 127).contains(octets[1])
            || octets[0] == 192 && octets[1] == 0 && octets[2] == 0
            || octets[0] == 192 && octets[1] == 0 && octets[2] == 2
            || octets[0] == 192 && octets[1] == 88 && octets[2] == 99
            || octets[0] == 198 && (octets[1] == 18 || octets[1] == 19)
            || octets[0] == 198 && octets[1] == 51 && octets[2] == 100
            || octets[0] == 203 && octets[1] == 0 && octets[2] == 113
            || octets[0] >= 240)
    }

    var isPrivateLAN: Bool {
        var address = address
        let octets = withUnsafeBytes(of: &address.s_addr) { Array($0) }
        return octets[0] == 10
            || octets[0] == 172 && (16 ... 31).contains(octets[1])
            || octets[0] == 192 && octets[1] == 168
    }
}

private struct IPv6Address {
    private var address = in6_addr()

    init?(_ value: String) {
        guard inet_pton(AF_INET6, value, &address) == 1 else { return nil }
    }

    var isTrustedDevice: Bool {
        var address = address
        let bytes = withUnsafeBytes(of: &address) { Array($0) }
        let allZero = bytes.allSatisfy { $0 == 0 }
        let loopback = bytes.dropLast().allSatisfy { $0 == 0 } && bytes.last == 1
        let multicast = bytes.first == 0xFF
        return !allZero && !loopback && !multicast
    }

    var isPublic: Bool {
        var address = address
        let bytes = withUnsafeBytes(of: &address) { Array($0) }
        guard isTrustedDevice, bytes.count == 16 else { return false }
        let segment0 = UInt16(bytes[0]) << 8 | UInt16(bytes[1])
        let segment1 = UInt16(bytes[2]) << 8 | UInt16(bytes[3])
        return segment0 & 0xE000 == 0x2000
            && !(segment0 == 0x2001 && segment1 <= 0x01FF)
            && !(segment0 == 0x2001 && segment1 == 0x0DB8)
            && segment0 != 0x2002
            && !(segment0 == 0x3FFF && segment1 & 0xF000 == 0)
    }

    var isPrivateLAN: Bool {
        var address = address
        let bytes = withUnsafeBytes(of: &address) { Array($0) }
        return bytes.count == 16 && bytes[0] & 0xFE == 0xFC
    }
}

private extension JSONEncoder {
    static var radroots: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        return encoder
    }
}
