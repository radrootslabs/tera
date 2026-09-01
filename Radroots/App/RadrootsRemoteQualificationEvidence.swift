import CryptoKit
import Foundation
import RadrootsKitBindings

struct RadrootsRemoteQualificationAuthorizationEvidence: Codable, Equatable {
  static let schema = "radroots.ios.remote-qualification.authorization-evidence.v1"
  static let digestDomain = "radroots.ios.remote-qualification.authorization-digest.v1\0"
  static let maximumEncodedBytes = 512

  let schema: String
  let schemaVersion: UInt16
  let authorizationDigestSHA256: String

  enum CodingKeys: String, CodingKey {
    case schema
    case schemaVersion = "schema_version"
    case authorizationDigestSHA256 = "authorization_digest_sha256"
  }

  init(runID: String, request: HostSigningRequest, signatureHex: String) {
    schema = Self.schema
    schemaVersion = 1
    authorizationDigestSHA256 = Self.digest(
      fields: [
        Data(runID.utf8),
        Data(request.operationId.utf8),
        Data(request.artifactId.utf8),
        Data(request.signerRequestId.utf8),
        Data(request.publicKey.utf8),
        Data(request.expectedEventId.utf8),
        request.eventIdDigest,
        Data(signatureHex.utf8),
      ]
    )
  }

  private static func digest(fields: [Data]) -> String {
    var hasher = SHA256()
    hasher.update(data: Data(digestDomain.utf8))
    for field in fields {
      var length = UInt64(field.count).bigEndian
      withUnsafeBytes(of: &length) { hasher.update(bufferPointer: $0) }
      hasher.update(data: field)
    }
    return hasher.finalize().map { String(format: "%02x", $0) }.joined()
  }
}

struct RadrootsRemoteQualificationEvidenceStore {
  static let evidenceFileName = "authorization-evidence.v1.json"
  static let legacyAuthorizationFileName = "radroots-remote-qualification-bud11.json"

  let runID: String
  let directory: URL
  let evidenceFile: URL
  let legacyAuthorizationFile: URL
  let fileManager: FileManager

  init(
    runID: String,
    temporaryRoot: URL,
    documentsRoot: URL,
    fileManager: FileManager = .default
  ) {
    self.runID = runID
    directory = temporaryRoot
      .appendingPathComponent("radroots-qualification-evidence", isDirectory: true)
      .appendingPathComponent(runID, isDirectory: true)
    evidenceFile = directory.appendingPathComponent(Self.evidenceFileName)
    legacyAuthorizationFile = documentsRoot.appendingPathComponent(
      Self.legacyAuthorizationFileName
    )
    self.fileManager = fileManager
  }

  static func current(fileManager: FileManager = .default) throws -> Self? {
    guard let environment = try RadrootsRemoteQualificationEnvironment.current() else {
      return nil
    }
    guard let documentsRoot = fileManager.urls(
      for: .documentDirectory,
      in: .userDomainMask
    ).first else {
      throw CocoaError(.fileNoSuchFile)
    }
    return Self(
      runID: environment.runID,
      temporaryRoot: fileManager.temporaryDirectory,
      documentsRoot: documentsRoot,
      fileManager: fileManager
    )
  }

  static func cleanupLegacyAuthorization(fileManager: FileManager = .default) throws {
    guard let documentsRoot = fileManager.urls(
      for: .documentDirectory,
      in: .userDomainMask
    ).first else {
      throw CocoaError(.fileNoSuchFile)
    }
    let legacy = documentsRoot.appendingPathComponent(Self.legacyAuthorizationFileName)
    if fileManager.fileExists(atPath: legacy.path) {
      try fileManager.removeItem(at: legacy)
    }
  }

  func prepare() throws {
    try cleanup()
  }

  func record(request: HostSigningRequest, signatureHex: String) throws {
    let evidence = RadrootsRemoteQualificationAuthorizationEvidence(
      runID: runID,
      request: request,
      signatureHex: signatureHex
    )
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    let data = try encoder.encode(evidence)
    guard data.count <= RadrootsRemoteQualificationAuthorizationEvidence.maximumEncodedBytes else {
      throw CocoaError(.fileWriteOutOfSpace)
    }
    try fileManager.createDirectory(
      at: directory,
      withIntermediateDirectories: true,
      attributes: [.posixPermissions: 0o700]
    )
    let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
    guard values.isDirectory == true, values.isSymbolicLink != true else {
      throw CocoaError(.fileWriteInvalidFileName)
    }
    try data.write(to: evidenceFile, options: [.atomic, .completeFileProtection])
    try fileManager.setAttributes([.posixPermissions: 0o600], ofItemAtPath: evidenceFile.path)
  }

  func cleanup() throws {
    try removeIfPresent(legacyAuthorizationFile)
    try removeIfPresent(evidenceFile)
    if fileManager.fileExists(atPath: directory.path) {
      try fileManager.removeItem(at: directory)
    }
  }

  private func removeIfPresent(_ url: URL) throws {
    if fileManager.fileExists(atPath: url.path) {
      try fileManager.removeItem(at: url)
    }
  }
}

enum RadrootsRemoteQualificationEvidence {
  static func prepare() throws -> RadrootsRemoteQualificationEvidenceStore? {
    try RadrootsRemoteQualificationEvidenceStore.cleanupLegacyAuthorization()
    let store = try RadrootsRemoteQualificationEvidenceStore.current()
    try store?.prepare()
    return store
  }

  static func recordBlossomAuthorization(
    request: HostSigningRequest,
    signatureHex: String
  ) throws {
    guard try RadrootsRemoteQualificationEnvironment.current()?.networkMode
      == .isolatedLoopback
    else { return }
    try RadrootsRemoteQualificationEvidenceStore.current()?.record(
      request: request,
      signatureHex: signatureHex
    )
  }
}
