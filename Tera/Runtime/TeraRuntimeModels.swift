import Foundation

enum TeraProtectedDataState: Sendable, Equatable {
  case available
  case unavailable
}

enum TeraRuntimeNetworkProfile: Sendable, Equatable {
  case publicNetwork
  case simulator
  case device
}

enum TeraBlossomHostKind: String, Codable, Sendable, Equatable {
  case native
  case simulator
  case physicalDevice = "physical_device"
}

enum TeraBlossomEndpointAuthority: String, Codable, Sendable, Equatable {
  case publicWebPKI = "public_web_pki"
  case loopbackDevelopment = "loopback_development"
  case privateNetworkDevelopment = "private_network_development"

  init?(runtimeValue: String) {
    switch runtimeValue {
    case "public_webpki": self = .publicWebPKI
    case "loopback_development": self = .loopbackDevelopment
    case "private_network_development": self = .privateNetworkDevelopment
    default: return nil
    }
  }
}

struct TeraBlossomEndpointConfiguration: Codable, Sendable, Equatable {
  let hostKind: TeraBlossomHostKind
  let endpointAuthority: TeraBlossomEndpointAuthority
  let primaryOrigin: String
  let fallbackOrigins: [String]
}

struct TeraBlossomConfigurationStatus: Sendable, Equatable {
  let schemaVersion: UInt16
  let hostKind: String
  let endpointAuthority: String
  let primaryOrigin: String
  let fallbackOrigins: [String]
  let configFingerprint: String
}

struct TeraBlossomEvidence: Sendable, Equatable {
  let schemaVersion: UInt16
  let origin: String
  let configFingerprint: String
  let state: String
  let lastSuccessfulState: String
  let transportSecurity: String
  let observedAtUnixMilliseconds: UInt64?
  let httpStatus: UInt16?
  let errorCode: String?
  let serverErrorCode: String?
  let errorPhase: String?
  let retryable: Bool
  let possibleOrphan: Bool
  let attempts: UInt8
}

struct TeraRuntimeAppMetadata: Sendable, Equatable {
  let bundleIdentifier: String
  let version: String
  let buildNumber: String
  let buildSHA: String?
}

enum TeraRuntimeSignerAvailability: Sendable, Equatable {
  case ready
  case busy
  case locked
  case unavailable
}

enum TeraRuntimeSigningPurpose: Sendable, Equatable {
  case nostrEvent
  case blossomUpload
}

struct TeraRuntimeSigningRequest: Sendable, Equatable {
  let operationID: String
  let signerRequestID: String
  let publicKeyHex: String
  let purpose: TeraRuntimeSigningPurpose
  let deadlineUnixMilliseconds: UInt64
  let digest: Data
}

enum TeraRuntimeSigningOutcome: Sendable, Equatable {
  case signed(signatureHex: String)
  case locked
  case cancelled
  case rejected
  case timedOut
  case unavailable
  case invalidated
  case failed
}

protocol TeraRuntimeSigner: Sendable {
  func availability() async -> TeraRuntimeSignerAvailability
  func sign(_ request: TeraRuntimeSigningRequest) async -> TeraRuntimeSigningOutcome
}

struct TeraRuntimeLaunchConfiguration: Sendable {
  let applicationSupportDirectory: String
  let publicKeyHex: String
  let sourceGenerationHex: String
  let sourceGenerationCreatedAtUnixMilliseconds: UInt64
  let protectedData: TeraProtectedDataState
  let networkProfile: TeraRuntimeNetworkProfile
  let writableRelays: [String]
  let blossom: TeraBlossomEndpointConfiguration?
  let app: TeraRuntimeAppMetadata
  let signerGeneration: String
  let signer: any TeraRuntimeSigner
  let adoptBootstrapSettings: Bool
}

extension TeraRuntimeLaunchConfiguration: Equatable {
  static func == (lhs: Self, rhs: Self) -> Bool {
    lhs.applicationSupportDirectory == rhs.applicationSupportDirectory
      && lhs.publicKeyHex == rhs.publicKeyHex
      && lhs.sourceGenerationHex == rhs.sourceGenerationHex
      && lhs.sourceGenerationCreatedAtUnixMilliseconds
        == rhs.sourceGenerationCreatedAtUnixMilliseconds
      && lhs.protectedData == rhs.protectedData
      && lhs.networkProfile == rhs.networkProfile
      && lhs.writableRelays == rhs.writableRelays
      && lhs.blossom == rhs.blossom
      && lhs.app == rhs.app
      && lhs.signerGeneration == rhs.signerGeneration
      && lhs.adoptBootstrapSettings == rhs.adoptBootstrapSettings
  }
}

struct TeraRuntimeIdentity: Sendable, Equatable {
  let publicKeyHex: String
  let hostSignerConfigured: Bool
}

enum TeraRelayAccess: Sendable, Equatable {
  case readOnly
  case readWrite

  var label: String {
    self == .readWrite ? "read write" : "read only"
  }
}

struct TeraRelayEndpointStatus: Sendable, Equatable {
  let url: String
  let access: TeraRelayAccess
  let readState: String
  let writeState: String
  let readLastAttemptUnixMilliseconds: UInt64?
  let writeLastAttemptUnixMilliseconds: UInt64?
  let readNextAttemptUnixMilliseconds: UInt64?
  let writeNextAttemptUnixMilliseconds: UInt64?
}

struct TeraRelayStatus: Sendable, Equatable {
  let profile: String
  let state: String
  let readAvailability: String
  let writeAvailability: String
  let relays: [TeraRelayEndpointStatus]
}

enum TeraSettingsIdentityLockState: Sendable, Equatable {
  case locked
  case unlocked
}

struct TeraSettingsIdentity: Sendable, Equatable, Identifiable {
  let id: String
  let publicKeyHex: String
}

struct TeraSettingsIdentityState: Sendable, Equatable {
  let identities: [TeraSettingsIdentity]
  let activeIdentityID: String?
  let lockState: TeraSettingsIdentityLockState
  let pendingImportOperationID: String?
}

enum TeraSettingsNetworkEnvironment: String, Sendable, Equatable, CaseIterable, Identifiable {
  case publicNetwork
  case simulator
  case physicalDevice

  var id: String {
    rawValue
  }
}

enum TeraRelayAccessPreference: String, Sendable, Equatable, CaseIterable, Identifiable {
  case readOnly
  case readWrite

  var id: String {
    rawValue
  }

  var label: String {
    self == .readWrite ? "Read and write" : "Read only"
  }
}

struct TeraRelayPreference: Sendable, Equatable, Identifiable {
  let id: UUID
  var url: String
  var access: TeraRelayAccessPreference

  init(id: UUID = UUID(), url: String, access: TeraRelayAccessPreference) {
    self.id = id
    self.url = url
    self.access = access
  }
}

enum TeraBlossomAuthorityPreference: String, Sendable, Equatable, CaseIterable, Identifiable {
  case publicWebPKI
  case loopbackDevelopment
  case privateNetworkDevelopment

  var id: String {
    rawValue
  }
}

struct TeraMobileSettings: Sendable, Equatable {
  let revision: UInt64
  let identity: TeraSettingsIdentityState
  var networkEnvironment: TeraSettingsNetworkEnvironment
  var relays: [TeraRelayPreference]
  var blossomAuthority: TeraBlossomAuthorityPreference
  var blossomPrimaryOrigin: String
  var blossomFallbackOrigins: [String]
  var allowCellularDownloads: Bool
  var allowCellularUploads: Bool
  var allowBackgroundTransfers: Bool
  var mediaCacheBytes: UInt64
  var mediaCacheArtifacts: UInt32
}

struct TeraReplaceSettings: Sendable, Equatable {
  let expectedRevision: UInt64
  let networkEnvironment: TeraSettingsNetworkEnvironment
  let relays: [TeraRelayPreference]
  let blossomAuthority: TeraBlossomAuthorityPreference
  let blossomPrimaryOrigin: String
  let blossomFallbackOrigins: [String]
  let allowCellularDownloads: Bool
  let allowCellularUploads: Bool
  let allowBackgroundTransfers: Bool
  let mediaCacheBytes: UInt64
  let mediaCacheArtifacts: UInt32
}

struct TeraSettingsTransition: Sendable, Equatable {
  let settings: TeraMobileSettings
  let runtimeRestartRequired: Bool
  let outboxRequeueRequired: Bool
  let mediaCacheInvalidationRequired: Bool
}

enum TeraIdentityCommandKind: Sendable, Equatable {
  case beginImport
  case completeImport
  case cancelImport
  case select
  case lock
  case unlock
  case recover
}

struct TeraIdentityCommand: Sendable, Equatable {
  let kind: TeraIdentityCommandKind
  let operationID: String?
  let identityID: String?
  let publicKeyHex: String?
}

struct TeraProfileMetadataInput: Sendable, Equatable {
  var name: String
  var displayName: String?
  var about: String?
  var picture: TeraPreparedMediaHandle?
  var banner: TeraPreparedMediaHandle?
  var nip05: String?
  var bot: Bool?
}

struct TeraProfileStatus: Sendable, Equatable, Identifiable {
  let id: String
  let revision: UInt64
  let authorPublicKey: String
  let state: TeraOutboxState
  let deliveryID: String?
  let createdAtUnixMilliseconds: UInt64
  let updatedAtUnixMilliseconds: UInt64
  let settlement: TeraOperationSettlement?

  var honestSummary: String {
    settlement?.summary ?? state.label
  }
}

struct TeraRuntimeSnapshot: Sendable, Equatable {
  let identity: TeraRuntimeIdentity
  let relay: TeraRelayStatus?
  let blossomConfiguration: TeraBlossomConfigurationStatus?
  let blossomEvidence: TeraBlossomEvidence?
  let crateName: String
  let crateVersion: String
  let isClosed: Bool
}

struct TeraRuntimeShutdownReceipt: Sendable, Equatable {
  let state: String
  let alreadyClosed: Bool

  static let alreadyStopped = Self(state: "closed", alreadyClosed: true)
}

enum TeraRuntimeOperationKind: String, Sendable, Equatable, Hashable {
  case startup
  case operation
  case reconfiguration
  case subscription
  case shutdown
}

struct TeraRuntimeDeadlinePolicy: Sendable, Equatable {
  let startupNanoseconds: UInt64
  let operationNanoseconds: UInt64
  let subscriptionNanoseconds: UInt64
  let shutdownNanoseconds: UInt64

  init(
    startupNanoseconds: UInt64 = 30_000_000_000,
    operationNanoseconds: UInt64 = 30_000_000_000,
    subscriptionNanoseconds: UInt64 = 10_000_000_000,
    shutdownNanoseconds: UInt64 = 10_000_000_000
  ) {
    precondition(startupNanoseconds > 0 && startupNanoseconds <= 120_000_000_000)
    precondition(operationNanoseconds > 0 && operationNanoseconds <= 120_000_000_000)
    precondition(subscriptionNanoseconds > 0 && subscriptionNanoseconds <= 60_000_000_000)
    precondition(shutdownNanoseconds > 0 && shutdownNanoseconds <= 60_000_000_000)
    self.startupNanoseconds = startupNanoseconds
    self.operationNanoseconds = operationNanoseconds
    self.subscriptionNanoseconds = subscriptionNanoseconds
    self.shutdownNanoseconds = shutdownNanoseconds
  }

  static let production = Self()
}

enum TeraRuntimeObservationState: Sendable, Equatable {
  case inactive
  case subscribing(attempt: UInt32)
  case active
  case retrying(attempt: UInt32, message: String)
  case stopped
}

enum TeraRuntimeObservationBackoff {
  static func sleep(attempt: UInt32) async throws {
    let exponent = min(attempt.saturatingSubtractingOne, 7)
    let delay = min(UInt64(250_000_000) << exponent, 30_000_000_000)
    try await Task.sleep(nanoseconds: delay)
  }
}

extension UInt32 {
  fileprivate var saturatingSubtractingOne: UInt32 {
    self == 0 ? 0 : self - 1
  }
}

struct TeraRuntimeFailure: Error, Sendable, Equatable {
  let schemaVersion: UInt16
  let code: String
  let category: String
  let retryable: Bool
  let recoveryActions: [String]
  let operationID: String?
  let capabilityID: String?
  let safeMessage: String

  static func local(operation: String, code: String, safeMessage: String) -> Self {
    Self(
      schemaVersion: 1,
      code: code,
      category: "runtime",
      retryable: false,
      recoveryActions: [],
      operationID: operation,
      capabilityID: nil,
      safeMessage: safeMessage
    )
  }
}

extension TeraRuntimeFailure: LocalizedError {
  var errorDescription: String? {
    TeraUserMessages.text(.runtimeOperationFailed)
  }
}

enum TeraRuntimeClientError: Error, Sendable, Equatable {
  case invalidBufferCapacity
  case notRunning
  case superseded
  case startup(TeraRuntimeFailure)
  case subscription(TeraRuntimeFailure)
  case status(TeraRuntimeFailure)
  case today(TeraRuntimeFailure)
  case add(TeraRuntimeFailure)
  case support(TeraRuntimeFailure)
  case shutdown(TeraRuntimeFailure)
}

extension TeraRuntimeClientError: LocalizedError {
  var errorDescription: String? {
    TeraUserMessages.text(for: self, fallback: .runtimeOperationFailed)
  }
}

struct TeraLocalNetwork: Sendable, Equatable, Hashable, Identifiable {
  let schemaVersion: UInt16
  let id: String
  let label: String
  let relayURLs: [String]
  let locality: String?
  let followedAuthors: [String]
  let generation: UInt64

  static func defaultContext(snapshot: TeraRuntimeSnapshot) -> Self {
    Self(
      schemaVersion: 1,
      id: "default",
      label: "Local network",
      relayURLs: snapshot.relay?.relays.map(\.url) ?? [],
      locality: nil,
      followedAuthors: [],
      generation: 1
    )
  }
}

enum TeraTodayCardType: String, CaseIterable, Sendable, Equatable, Hashable {
  case update
  case photoUpdate
  case ask
  case event
  case foodAvailability

  var label: String {
    switch self {
    case .update: "Update"
    case .photoUpdate: "Photo update"
    case .ask: "Ask"
    case .event: "Event"
    case .foodAvailability: "Food availability"
    }
  }

  var addCommandType: TeraAddCommandType {
    switch self {
    case .update: .createUpdate
    case .photoUpdate: .createPhotoUpdate
    case .ask: .createAsk
    case .event: .createEvent
    case .foodAvailability: .createFoodAvailability
    }
  }
}

enum TeraMediaVerificationState: String, Sendable, Equatable, Hashable {
  case pending
  case verified
  case failed
  case unavailable
}

struct TeraMediaReference: Sendable, Equatable, Hashable, Identifiable {
  let referenceFingerprint: String
  let url: String
  let sha256: String?
  let mediaType: String?
  let width: UInt32?
  let height: UInt32?
  let byteSize: UInt64?
  let alt: String?
  let verification: TeraMediaVerificationState

  var id: String {
    referenceFingerprint
  }

  var verifiedArtifactID: String? {
    guard verification == .verified,
      let sha256,
      sha256.count == 64,
      sha256.allSatisfy({ $0.isHexDigit && !$0.isUppercase })
    else { return nil }
    return sha256
  }
}

struct TeraVerifiedMediaArtifact: Sendable, Equatable {
  let artifactID: String
  let bytes: Data
  let byteSize: UInt64
  let mediaType: String
  let width: UInt32
  let height: UInt32

  init?(
    artifactID: String,
    bytes: Data,
    byteSize: UInt64,
    mediaType: String,
    width: UInt32,
    height: UInt32
  ) {
    guard artifactID.count == 64,
      artifactID.allSatisfy({ $0.isHexDigit && !$0.isUppercase }),
      !bytes.isEmpty,
      UInt64(bytes.count) == byteSize,
      ["image/gif", "image/jpeg", "image/png", "image/webp"].contains(mediaType),
      width > 0,
      height > 0
    else { return nil }
    self.artifactID = artifactID
    self.bytes = bytes
    self.byteSize = byteSize
    self.mediaType = mediaType
    self.width = width
    self.height = height
  }
}

struct TeraProfileSummary: Sendable, Equatable, Hashable {
  let authorPublicKey: String
  let name: String?
  let displayName: String?
  let about: String?
  let picture: TeraMediaReference?
  let banner: TeraMediaReference?
  let nip05: String?
  let website: String?
  let lightningAddress: String?

  var preferredName: String {
    for candidate in [displayName, name] {
      if let candidate, !candidate.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
        return candidate
      }
    }
    guard authorPublicKey.count > 16 else { return authorPublicKey }
    return "\(authorPublicKey.prefix(8))…\(authorPublicKey.suffix(8))"
  }
}

enum TeraThreadEntryType: String, Sendable, Equatable, Hashable {
  case profile
  case reply
  case comment
  case deletion
}

struct TeraThreadEntry: Sendable, Equatable, Hashable, Identifiable {
  let id: String
  let authorPublicKey: String
  let content: String
  let authoredAtUnixSeconds: UInt64
  let type: TeraThreadEntryType
  let root: String
  let parentEventID: String
  let authorProfile: TeraProfileSummary?
}

enum TeraCardLifecycleState: String, Sendable, Equatable, Hashable {
  case active
  case sold
  case past
}

struct TeraTodayCard: Sendable, Equatable, Hashable, Identifiable {
  let id: String
  let type: TeraTodayCardType
  let sourceEventID: String
  let sourceAddress: String?
  let authorPublicKey: String
  let contractID: String
  let title: String?
  let content: String
  let authoredAtUnixSeconds: UInt64
  let effectiveAtUnixSeconds: UInt64
  let eventStartUnixSeconds: UInt64?
  let eventEndUnixSeconds: UInt64?
  let location: String?
  let priceAmount: String?
  let priceCurrency: String?
  let priceUnit: String?
  let quantity: String?
  let foodSummary: String?
  let foodPublishedAtUnixSeconds: UInt64?
  let foodStatus: String?
  let contextRank: UInt8
  let inclusionReason: String
  let media: [TeraMediaReference]
  let lifecycle: TeraCardLifecycleState
  let rankDigest: String?
  let authorProfile: TeraProfileSummary?
  let thread: [TeraThreadEntry]
  let localOperationID: String?
  let localOperationState: String?

  var authorName: String {
    authorProfile?.preferredName
      ?? TeraProfileSummary(
        authorPublicKey: authorPublicKey,
        name: nil,
        displayName: nil,
        about: nil,
        picture: nil,
        banner: nil,
        nip05: nil,
        website: nil,
        lightningAddress: nil
      ).preferredName
  }

  var priceSummary: String? {
    guard let priceAmount, let priceCurrency, let priceUnit else { return nil }
    return "\(priceAmount) \(priceCurrency)/\(priceUnit)"
  }

  var accessibilitySummary: String {
    var parts = [type.label, "by \(authorName)"]
    if let title {
      parts.append(title)
    }
    if !content.isEmpty {
      parts.append(content)
    }
    if let priceSummary {
      parts.append(priceSummary)
    }
    if lifecycle != .active {
      parts.append(lifecycle.rawValue)
    }
    if let localOperationState {
      parts.append(localOperationState)
    }
    return parts.joined(separator: ", ")
  }

  var retractionTargetKind: UInt32? {
    switch type {
    case .update, .photoUpdate, .ask:
      guard sourceAddress == nil else { return nil }
      return 1
    case .event:
      guard let kind = sourceAddressKind, kind == 31922 || kind == 31923 else { return nil }
      return kind
    case .foodAvailability:
      guard sourceAddressKind == 30402 else { return nil }
      return 30402
    }
  }

  private var sourceAddressKind: UInt32? {
    guard let sourceAddress,
      let rawKind = sourceAddress.split(separator: ":", maxSplits: 1).first,
      let kind = UInt32(rawKind)
    else {
      return nil
    }
    return kind
  }
}

enum TeraTodayProjectionUpdate: Sendable, Equatable {
  case incremental
  case rebuild
}

enum TeraSearchResultType: Sendable, Equatable, Hashable {
  case card
  case profile
}

struct TeraSearchResult: Sendable, Equatable, Hashable, Identifiable {
  let type: TeraSearchResultType
  let id: String
  let card: TeraTodayCard?
  let profile: TeraProfileSummary?
}

struct TeraMeSnapshot: Sendable, Equatable {
  let publicKey: String
  let profile: TeraProfileSummary?
  let cards: [TeraTodayCard]
}

enum TeraAddCommandType: String, CaseIterable, Sendable, Equatable, Hashable, Identifiable {
  case createUpdate
  case createPhotoUpdate
  case createAsk
  case createEvent
  case createFoodAvailability

  var id: String {
    rawValue
  }

  var label: String {
    switch self {
    case .createUpdate: "Update"
    case .createPhotoUpdate: "Photo update"
    case .createAsk: "Ask"
    case .createEvent: "Event"
    case .createFoodAvailability: "Food availability"
    }
  }

  var todayCardType: TeraTodayCardType {
    switch self {
    case .createUpdate: .update
    case .createPhotoUpdate: .photoUpdate
    case .createAsk: .ask
    case .createEvent: .event
    case .createFoodAvailability: .foodAvailability
    }
  }
}

enum TeraAddFieldKind: Sendable, Equatable, Hashable {
  case text
  case multilineText
  case date
  case dateTime
  case decimal
  case choice
  case location
  case media
}

struct TeraAddField: Sendable, Equatable, Hashable, Identifiable {
  let schemaVersion: UInt16
  let id: String
  let label: String
  let kind: TeraAddFieldKind
  let required: Bool
  let choices: [String]
  let maxBytes: UInt64?
  let maxItems: UInt16?
}

struct TeraAddSchema: Sendable, Equatable, Hashable, Identifiable {
  let schemaVersion: UInt16
  let commandType: TeraAddCommandType
  let label: String
  let fields: [TeraAddField]

  var id: TeraAddCommandType {
    commandType
  }
}

enum TeraProductSurfaceContractError: LocalizedError, Sendable, Equatable {
  case invalidAddSchemaInventory

  var errorDescription: String? {
    "The Add product contract does not match this app."
  }
}

enum TeraProductSurfaceContract {
  private struct MediaProfile: Equatable {
    let required: Bool
    let maximum: UInt16
  }

  static let supportingSurfaceIDs = ["context_picker", "search", "me", "settings"]

  static var snapshot: String {
    let cards = TeraTodayCardType.allCases.map(\.rawValue).joined(separator: ",")
    let commands = TeraAddCommandType.allCases.map(\.rawValue).joined(separator: ",")
    let supporting = supportingSurfaceIDs.joined(separator: ",")
    return "today=\(cards)|add=\(commands)|support=\(supporting)"
  }

  static func validate(schemas: [TeraAddSchema]) throws -> [TeraAddSchema] {
    let expectedCommands = TeraAddCommandType.allCases
    guard schemas.map(\.commandType) == expectedCommands,
      schemas.allSatisfy({ $0.schemaVersion == 1 && $0.label == $0.commandType.label }),
      schemas.allSatisfy({ schema in
        schema.fields.allSatisfy { $0.schemaVersion == 1 }
          && Set(schema.fields.map(\.id)).count == schema.fields.count
      }),
      schemas.allSatisfy({ $0.fields.map(\.id) == expectedFieldIDs(for: $0.commandType) }),
      schemas.allSatisfy({ $0.fields.map(\.kind) == expectedFieldKinds(for: $0.commandType) }),
      schemas.allSatisfy({
        requiredFieldIDs(in: $0) == expectedRequiredFieldIDs(for: $0.commandType)
      }),
      schemas.allSatisfy({ mediaProfile(in: $0) == expectedMediaProfile(for: $0.commandType) }),
      schemas.first(where: { $0.commandType == .createFoodAvailability })?
        .fields.first(where: { $0.id == "unit" })?.choices
        == ["g", "kg", "lb", "oz", "each", "dozen", "bunch", "punnet", "bag", "basket"]
    else {
      throw TeraProductSurfaceContractError.invalidAddSchemaInventory
    }
    return schemas
  }

  private static func expectedFieldIDs(for command: TeraAddCommandType) -> [String] {
    switch command {
    case .createUpdate:
      ["content"]
    case .createPhotoUpdate, .createAsk:
      ["content", "media"]
    case .createEvent:
      ["identifier", "title", "content", "event_start", "event_end", "location", "media"]
    case .createFoodAvailability:
      [
        "identifier", "title", "summary", "content", "location", "price_amount", "currency",
        "unit", "quantity", "media",
      ]
    }
  }

  private static func expectedRequiredFieldIDs(for command: TeraAddCommandType) -> Set<String> {
    switch command {
    case .createUpdate:
      ["content"]
    case .createPhotoUpdate:
      ["content", "media"]
    case .createAsk:
      ["content"]
    case .createEvent:
      ["identifier", "title", "event_start"]
    case .createFoodAvailability:
      ["identifier", "title", "summary", "content", "location", "price_amount", "currency", "unit"]
    }
  }

  private static func expectedFieldKinds(
    for command: TeraAddCommandType
  ) -> [TeraAddFieldKind] {
    switch command {
    case .createUpdate:
      [.multilineText]
    case .createPhotoUpdate, .createAsk:
      [.multilineText, .media]
    case .createEvent:
      [.text, .text, .multilineText, .dateTime, .dateTime, .location, .media]
    case .createFoodAvailability:
      [
        .text, .text, .text, .multilineText, .location, .decimal, .choice, .choice, .decimal,
        .media,
      ]
    }
  }

  private static func requiredFieldIDs(in schema: TeraAddSchema) -> Set<String> {
    Set(schema.fields.lazy.filter(\.required).map(\.id))
  }

  private static func expectedMediaProfile(
    for command: TeraAddCommandType
  ) -> MediaProfile? {
    switch command {
    case .createUpdate:
      nil
    case .createPhotoUpdate:
      MediaProfile(required: true, maximum: 20)
    case .createAsk:
      MediaProfile(required: false, maximum: 20)
    case .createEvent:
      MediaProfile(required: false, maximum: 1)
    case .createFoodAvailability:
      MediaProfile(required: false, maximum: 20)
    }
  }

  private static func mediaProfile(
    in schema: TeraAddSchema
  ) -> MediaProfile? {
    let fields = schema.fields.filter { $0.kind == .media }
    guard fields.count == 1, let field = fields.first, let maximum = field.maxItems else {
      return nil
    }
    return MediaProfile(required: field.required, maximum: maximum)
  }
}

enum TeraEventTiming: String, Sendable, Equatable, Hashable, CaseIterable, Identifiable {
  case allDay
  case timed

  var id: String {
    rawValue
  }

  var label: String {
    self == .allDay ? "All day" : "Specific time"
  }
}

struct TeraPreparedMedia: Sendable, Equatable, Hashable, Identifiable {
  let opaqueReference: String
  let remoteURL: String?
  let sha256: String
  let mediaType: String
  let byteSize: UInt64
  let width: UInt32
  let height: UInt32
  var alt: String
  let preparedAtUnixSeconds: UInt64

  var id: String {
    opaqueReference
  }
}

struct TeraAddForm: Sendable, Equatable, Hashable {
  var commandType: TeraAddCommandType
  var content: String = ""
  var identifier: String?
  var title: String?
  var summary: String?
  var location: String?
  var eventTiming: TeraEventTiming?
  var eventStartDate: String?
  var eventEndDate: String?
  var eventStartUnixSeconds: UInt64?
  var eventEndUnixSeconds: UInt64?
  var eventTimezone: String?
  var priceAmount: String?
  var currency: String?
  var unit: String?
  var quantity: String?
  var foodPublishedAtUnixSeconds: UInt64?
  var foodStatus: String?
  var media: [TeraPreparedMedia] = []

  static func empty(_ type: TeraAddCommandType = .createUpdate) -> Self {
    var form = Self(commandType: type)
    if type == .createEvent {
      form.eventTiming = .timed
      form.eventTimezone = TimeZone.current.identifier
    }
    return form
  }
}

struct TeraAddRuntimeInput: Sendable, Equatable {
  let form: TeraAddForm
  let media: [TeraPreparedMediaHandle]
}

enum TeraDraftKind: String, Sendable, Equatable, Hashable {
  case add
  case retraction
}

enum TeraOutboxState: String, Sendable, Equatable, Hashable {
  case draft
  case mediaPreparing
  case mediaUploading
  case readyToSign
  case signing
  case signed
  case queued
  case delivering
  case partiallyDelivered
  case retryable
  case terminal
  case cancelled
  case complete

  var isEditable: Bool {
    self == .draft || self == .mediaPreparing
  }

  var canAdvance: Bool {
    self == .queued || self == .retryable || self == .partiallyDelivered
  }

  var canCancel: Bool {
    ![.cancelled, .complete, .terminal].contains(self)
  }

  var label: String {
    rawValue
      .replacingOccurrences(of: "([a-z])([A-Z])", with: "$1 $2", options: .regularExpression)
      .capitalized
  }
}

enum TeraDraftMediaStage: String, Sendable, Equatable, Hashable {
  case pending
  case preparing
  case uploading
  case verified
  case failed
  case orphaned
}

struct TeraDraftMediaStatus: Sendable, Equatable, Hashable {
  let url: String
  let stage: TeraDraftMediaStage
  let uploadAttempts: UInt8
  let verifiedAtUnixMilliseconds: UInt64?
  let possibleOrphan: Bool
  let orphanReasonCode: String?
  let orphanRecordedAtUnixMilliseconds: UInt64?
}

struct TeraOperationSettlement: Sendable, Equatable, Hashable {
  let artifacts: UInt16
  let signed: UInt16
  let admitted: UInt16
  let pending: UInt16
  let retryable: UInt16
  let indeterminate: UInt16
  let failedTerminal: UInt16
  let cancelled: UInt16
  let deliveryPlans: UInt16
  let deliverySatisfied: UInt16
  let deliveryPending: UInt16
  let deliveryRetryable: UInt16
  let deliveryExhausted: UInt16
  let deliveryFailedTerminal: UInt16
  let deliveryCancelled: UInt16

  var summary: String {
    if deliverySatisfied > 0 {
      return "Delivered"
    }
    if deliveryRetryable > 0 || retryable > 0 {
      return "Saved; delivery can be retried"
    }
    if indeterminate > 0 {
      return "Delivery outcome is not yet known"
    }
    if failedTerminal > 0 || deliveryFailedTerminal > 0 {
      return "Delivery failed"
    }
    if admitted > 0 {
      return "Published locally; relay delivery is pending"
    }
    if signed > 0 {
      return "Signed; local publication is pending"
    }
    return "Waiting"
  }
}

struct TeraDraftStatus: Sendable, Equatable, Hashable, Identifiable {
  let id: String
  let revision: UInt64
  let authorPublicKey: String
  let kind: TeraDraftKind
  let commandType: TeraAddCommandType
  let form: TeraAddForm?
  let state: TeraOutboxState
  let cardID: String
  let operationID: String?
  let createdAtUnixMilliseconds: UInt64
  let updatedAtUnixMilliseconds: UInt64
  let media: [TeraDraftMediaStatus]
  let settlement: TeraOperationSettlement?
  let isRevision: Bool

  var honestSummary: String {
    settlement?.summary ?? state.label
  }
}

struct TeraRetractionDraftInput: Sendable, Equatable, Hashable {
  let commandType: TeraAddCommandType
  let targetCardID: String
  let targetEventID: String
  let targetKind: UInt32
  let targetAddress: String?
  let reason: String
}

struct TeraRevisionTarget: Sendable, Equatable, Hashable {
  let cardID: String
  let sourceEventID: String
  let sourceAddress: String?
  let authorPublicKey: String
}

enum TeraRevisionPolicy: Sendable, Equatable, Hashable {
  case replaceThenRetract
  case addressableReplacement
}

enum TeraRevisionPhase: Sendable, Equatable, Hashable {
  case replacementPending
  case replacementFailed
  case retractionPending
  case complete
  case partialEffect
  case cancelled
}

struct TeraRevisionStatus: Sendable, Equatable {
  let operationID: String
  let replacement: TeraDraftStatus
  let retraction: TeraDraftStatus?
  let policy: TeraRevisionPolicy
  let phase: TeraRevisionPhase

  var honestSummary: String {
    switch phase {
    case .replacementPending: "Replacement saved for delivery"
    case .replacementFailed: "Replacement failed; the original remains"
    case .retractionPending: "Replacement published; retraction is pending"
    case .complete: "Revision complete"
    case .partialEffect: "Replacement published; retraction did not complete"
    case .cancelled: "Revision cancelled"
    }
  }
}

struct TeraBlossomUploadIntent: Sendable, Equatable {
  let draftID: String
  let expectedRevision: UInt64
  let media: TeraPreparedMediaHandle
}

struct TeraNativeUploadJob: Sendable, Equatable {
  let operationID: String
  let draft: TeraDraftStatus
  let remoteURL: String
  let authorizationHeader: String
  let expectedSHA256: String
  let mediaType: String
  let byteSize: UInt64
}

struct TeraNativeUploadCompletion: Sendable, Equatable {
  let draftID: String
  let expectedRevision: UInt64
  let media: TeraPreparedMediaHandle
  let statusCode: UInt16
  let responseMediaType: String?
  let responseContentEncoding: String?
  let responseBody: Data
}

enum TeraRuntimeLifecycle: Sendable, Equatable {
  case stopped
  case starting(generation: TeraSessionGeneration)
  case running(generation: TeraSessionGeneration)
  case stopping(generation: TeraSessionGeneration)
  case failed(generation: TeraSessionGeneration, failure: TeraRuntimeFailure)
}
