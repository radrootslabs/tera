import CryptoKit
import Foundation
import RadrootsKit
import TeraKitBindings

/// Recovery metadata and immutable media stay with the existing file owner.
struct TeraRestoreFiles {
  let roots: RadrootsAppleFileRoots
  let publicKey: String

  static func readGuard(applicationSupportDirectory: String, publicKey: String) throws -> Data? {
    let root = URL(fileURLWithPath: applicationSupportDirectory, isDirectory: true)
    let roots = try RadrootsAppleFileRoots(appIdentifier: "tera.restore.read", dataRoot: root, cacheRoot: root, temporaryRoot: root)
    return try Self(roots: roots, publicKey: publicKey).readGuard()
  }

  func readGuard() throws -> Data? {
    let relative = try applicationRestoreGuardPath(publicKey: publicKey)
    do {
      guard case let .inline(bytes) = try RadrootsAppleFileAccess(roots: roots).read(
        RadrootsFileReference(scope: .data, relativePath: relative), mode: .inline(maxBytes: Int(applicationRestoreGuardLimit()))
      ) else { throw TeraLocalRestoreHost.failure("restore_recovery_required") }
      let guardRecord = try validateApplicationRestoreGuard(bytes: bytes)
      guard guardRecord.relativePath == relative, guardRecord.request.backup.publicKey == publicKey else {
        throw TeraLocalRestoreHost.failure("restore_identity_mismatch")
      }
      return bytes
    } catch RadrootsAppleFileError.notFound {
      return nil
    }
  }

  func arm(_ guardRecord: FfiRestoreGuard) throws {
    let validated = try validateApplicationRestoreGuard(bytes: guardRecord.bytes)
    guard validated == guardRecord, validated.request.backup.publicKey == publicKey else {
      throw TeraLocalRestoreHost.failure("restore_identity_mismatch")
    }
    let relative = validated.relativePath
    let owner = roots.dataRoot.appendingPathComponent(relative).deletingLastPathComponent()
    let access = try RadrootsAppleFileAccess(roots: RadrootsAppleFileRoots(
      appIdentifier: roots.appIdentifier, dataRoot: roots.dataRoot, cacheRoot: roots.cacheRoot,
      temporaryRoot: roots.temporaryRoot, stagedBlobsRoot: owner
    ))
    // The admitted owner directory supplies custody metadata before immutable
    // installation. Foundation cannot change metadata on a read-only guard.
    try TeraBackupFiles.protect(owner)
    try access.installStagedBlob(validated.bytes, reference: RadrootsStagedBlobReference(
      blobID: URL(fileURLWithPath: relative).lastPathComponent, sizeBytes: validated.bytes.count
    ))
    guard try readGuard() == validated.bytes else { throw TeraLocalRestoreHost.failure("restore_recovery_required") }
  }

  func restoreMedia(_ manifest: FfiBackupManifest) throws {
    try validateApplicationBackupMedia(request: manifest.request, media: manifest.media)
    guard manifest.request.publicKey == publicKey else { throw TeraLocalRestoreHost.failure("restore_identity_mismatch") }
    let access = RadrootsAppleFileAccess(roots: roots)
    for item in manifest.media {
      try Task.checkCancellation()
      guard let size = Int(exactly: item.byteLength), size > 0,
        item.byteLength <= applicationBackupLimits().mediaBytes
      else { throw TeraLocalRestoreHost.failure("restore_capacity_exceeded") }
      let reference = RadrootsFileReference(scope: .data, relativePath: "backups/\(publicKey)/staged_blob_leases/\(item.leaseIdentifier)")
      guard case let .inline(bytes) = try access.read(reference, mode: .inline(maxBytes: size)), bytes.count == size,
        SHA256.hash(data: bytes).map({ String(format: "%02x", $0) }).joined() == item.sha256
      else { throw TeraLocalRestoreHost.failure("restore_media_unavailable") }
      let blob = try RadrootsStagedBlobReference(blobID: item.sha256, sizeBytes: size)
      try access.installStagedBlob(bytes, reference: blob)
    }
  }
}
