import CryptoKit
import Foundation
import RadrootsKit

/// Prepared photos live with application data. Existing temporary copies stay
/// untouched and are migrated lazily when an acknowledged reference is opened.
enum TeraDurableMediaRoots {
  static func selectingStaging(in base: RadrootsAppleFileRoots) throws -> RadrootsAppleFileRoots {
    try RadrootsAppleFileRoots(
      appIdentifier: base.appIdentifier, dataRoot: base.dataRoot, cacheRoot: base.cacheRoot,
      temporaryRoot: base.temporaryRoot, logsRoot: base.logsRoot,
      stagedBlobsRoot: base.dataRoot.appendingPathComponent("staged_blobs", isDirectory: true)
    )
  }

  static func restoreLegacyBlob(_ blob: RadrootsStagedBlobReference, roots: RadrootsAppleFileRoots) throws {
    let access = RadrootsAppleFileAccess(roots: roots)
    do {
      try validate(access.readStagedBlob(blob), reference: blob)
      return
    } catch RadrootsAppleFileError.notFound {
      // Only definitive absence permits the legacy copy to be installed.
      // Conflicting, protected or unsafe destinations require recovery.
    }
    let legacy = try RadrootsAppleFileRoots(
      appIdentifier: roots.appIdentifier, dataRoot: roots.dataRoot, cacheRoot: roots.cacheRoot,
      temporaryRoot: roots.temporaryRoot, logsRoot: roots.logsRoot
    )
    guard legacy.stagedBlobsRoot != roots.stagedBlobsRoot else { throw RadrootsAppleFileError.notFound }
    let bytes = try RadrootsAppleFileAccess(roots: legacy).readStagedBlob(blob)
    try validate(bytes, reference: blob)
    // Preserve the legacy copy and reconcile a completed install by its exact
    // existing reference on restart, without introducing a second journal.
    try access.installStagedBlob(bytes, reference: blob)
  }

  private static func validate(_ bytes: Data, reference blob: RadrootsStagedBlobReference) throws {
    let digest = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    guard digest == blob.blobID else {
      throw TeraRuntimeFailure.local(operation: "add.media.migrate", code: "ios.add.media_corrupt",
                                     safeMessage: "A prepared photo could not be verified on this device.")
    }
  }
}
