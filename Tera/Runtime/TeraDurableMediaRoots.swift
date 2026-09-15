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
    let destination = try roots.stagedBlobURL(for: blob)
    guard !FileManager.default.fileExists(atPath: destination.path) else { return }
    let legacy = try RadrootsAppleFileRoots(
      appIdentifier: roots.appIdentifier, dataRoot: roots.dataRoot, cacheRoot: roots.cacheRoot,
      temporaryRoot: roots.temporaryRoot, logsRoot: roots.logsRoot
    )
    guard legacy.stagedBlobsRoot != roots.stagedBlobsRoot else { return }
    let bytes = try RadrootsAppleFileAccess(roots: legacy).readStagedBlob(blob)
    let digest = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    guard digest == blob.blobID else {
      throw TeraRuntimeFailure.local(operation: "add.media.migrate", code: "ios.add.media_corrupt",
                                     safeMessage: "A prepared photo could not be verified on this device.")
    }
    // The generic producer supports opaque IDs, so verify the application's
    // content identity before atomic installation. Preserve the legacy copy.
    try RadrootsAppleFileAccess(roots: roots).installStagedBlob(bytes, reference: blob)
  }
}
