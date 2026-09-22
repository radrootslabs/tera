import Foundation
import RadrootsKit
import TeraKitBindings
import UIKit

/// Explicit local backup only. Credential export and restore are separate ports.
actor TeraLocalBackupHost: TeraBackupHost {
  let roots: RadrootsAppleFileRoots
  let publicKey: String
  let generation: String
  let protectedData: @Sendable () async -> Bool

  init(roots: RadrootsAppleFileRoots, publicKey: String, generation: String,
       protectedData: @escaping @Sendable () async -> Bool = currentProtectedData) throws
  {
    self.roots = try TeraDurableMediaRoots.selectingStaging(in: roots)
    self.publicKey = publicKey
    self.generation = generation
    self.protectedData = protectedData
  }

  /// The complete FFI call retains admission, including every callback and any
  /// canceled callback still finishing. Persistent markers also guard restart.
  func capture(runtime: TeraRuntime, request: FfiBackupRequest) async throws -> FfiBackupManifest {
    try await admit(request)
    let use = try reserve()
    defer { withExtendedLifetime(use) {} }
    return try await runtime.captureApplicationBackup(request: request, host: self)
  }

  func prepare(request: FfiBackupRequest) async throws {
    try await admit(request)
    let use = try reserve()
    defer { withExtendedLifetime(use) {} }
    do { try files.prepare() } catch { throw Self.failure("backup_unavailable") }
  }

  func loadCandidate(request: FfiBackupRequest) async throws -> Data? {
    try await admit(request)
    let use = try reserve()
    defer { withExtendedLifetime(use) {} }
    do {
      return try files.read(id: request.backupId)
    } catch { throw Self.failure("backup_verification_failed") }
  }

  func retainMedia(request: FfiBackupRequest, media: [FfiBackupMedia]) async throws -> [FfiBackupMedia] {
    try await admit(request)
    let use = try reserve()
    defer { withExtendedLifetime(use) {} }
    try retain(request: request, media: media)
    return media
  }

  func persistCandidate(manifest: FfiBackupManifest) async throws {
    try await admit(manifest.request)
    let use = try reserve()
    defer { withExtendedLifetime(use) {} }
    do {
      try retain(request: manifest.request, media: manifest.media)
      try files.install(manifest.manifest, id: manifest.request.backupId, completed: false)
    } catch { throw Self.failure("backup_publication_incomplete") }
  }

  func publishComplete(manifest: FfiBackupManifest) async throws {
    try await admit(manifest.request)
    let use = try reserve()
    defer { withExtendedLifetime(use) {} }
    do {
      try retain(request: manifest.request, media: manifest.media)
      guard try files.read(id: manifest.request.backupId) == manifest.manifest else {
        throw Self.failure("backup_conflict")
      }
      try files.install(manifest.manifest, id: manifest.request.backupId, completed: true)
    } catch { throw Self.failure("backup_publication_incomplete") }
  }

  private var files: TeraBackupFiles {
    TeraBackupFiles(roots: roots, publicKey: publicKey)
  }

  private func reserve() throws -> RadrootsFileUseReservation {
    do {
      return try TeraMediaProcessUse.admit(root: roots.dataRoot)
    } catch { throw Self.failure("backup_unavailable") }
  }

  private func admit(_ request: FfiBackupRequest) async throws {
    try validateApplicationBackupRequest(request: request)
    guard request.publicKey == publicKey else { throw Self.failure("backup_identity_mismatch") }
    guard request.sourceGeneration == generation else { throw Self.failure("backup_generation_mismatch") }
    guard await protectedData() else { throw Self.failure("backup_unavailable") }
    guard !Task.isCancelled else { throw Self.failure("backup_publication_incomplete") }
  }

  private func retain(request: FfiBackupRequest, media: [FfiBackupMedia]) throws {
    try validateApplicationBackupMedia(request: request, media: media)
    do {
      let access = try files.mediaAccess()
      for item in media {
        try Task.checkCancellation()
        guard let size = Int(exactly: item.byteLength) else { throw Self.failure("backup_media_unavailable") }
        let blob = try RadrootsStagedBlobReference(blobID: item.sha256, sizeBytes: size)
        _ = try access.leaseStagedBlob(blob, expectedSHA256: item.sha256, identifier: item.leaseIdentifier)
      }
    } catch { throw Self.failure("backup_media_unavailable") }
  }

  nonisolated static func failure(_ code: String) -> TeraAppError {
    .Failure(report: TeraErrorRecord(schemaVersion: 1, code: code, category: "backup", retryable: false,
                                     recoveryActions: ["inspect_local_stores"], operationId: nil, capabilityId: nil,
                                     safeMessage: "The local backup could not be completed."))
  }

  private static func currentProtectedData() async -> Bool {
    await MainActor.run { UIApplication.shared.isProtectedDataAvailable }
  }
}
