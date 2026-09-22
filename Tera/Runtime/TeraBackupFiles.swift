import Foundation
import RadrootsKit
import TeraKitBindings

/// Uses the existing durable file owner. It never reads or copies live DB files.
struct TeraBackupFiles {
  let roots: RadrootsAppleFileRoots
  let publicKey: String

  func prepare() throws {
    // The caller has validated the binding and observed protected data. Marker
    // installation creates directories through the no-follow durable owner.
    for directory in ["sqlite", "manifests", "staged_blob_leases"] {
      try prepare(directory: directory)
    }
  }

  /// Metadata belongs on the admitted directory before the owner installs
  /// read-only leases. Original media remains in the canonical staging root.
  func mediaAccess() throws -> RadrootsAppleFileAccess {
    try prepare(directory: "staged_blob_leases")
    return try RadrootsAppleFileAccess(roots: RadrootsAppleFileRoots(
      appIdentifier: roots.appIdentifier, dataRoot: accountDirectory,
      cacheRoot: roots.cacheRoot, temporaryRoot: roots.temporaryRoot,
      stagedBlobsRoot: roots.stagedBlobsRoot
    ))
  }

  func read(id: String, completed: Bool = false) throws -> Data? {
    let relative = "backups/\(publicKey)/manifests/\(name(id: id, completed: completed))"
    do {
      guard case let .inline(data) = try RadrootsAppleFileAccess(roots: roots).read(
        RadrootsFileReference(scope: .data, relativePath: relative),
        mode: .inline(maxBytes: Int(applicationBackupLimits().manifestBytes))
      ) else { throw TeraLocalBackupHost.failure("backup_verification_failed") }
      return data
    } catch RadrootsAppleFileError.notFound {
      return nil
    }
  }

  func install(_ bytes: Data, id: String, completed: Bool) throws {
    guard !bytes.isEmpty, bytes.count <= applicationBackupLimits().manifestBytes else {
      throw TeraLocalBackupHost.failure("backup_capacity_exceeded")
    }
    // Opaque IDs are create-only: different bytes cannot replace prior evidence.
    try access(directory: "manifests").installStagedBlob(bytes, reference: RadrootsStagedBlobReference(
      blobID: name(id: id, completed: completed), sizeBytes: bytes.count
    ))
  }

  /// Applies local custody policy only to exact objects just admitted by the
  /// file owner. This never walks or recursively repairs unrelated paths.
  static func protect(_ admittedURL: URL) throws {
    let attributes = try admittedURL.resourceValues(forKeys: [.isSymbolicLinkKey])
    guard attributes.isSymbolicLink == false else { throw RadrootsAppleFileError.invalidRequest }
    var url = admittedURL
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    try url.setResourceValues(values)
    #if os(iOS)
    try FileManager.default.setAttributes(
      [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: url.path
    )
    #endif
  }

  private var accountDirectory: URL {
    roots.dataRoot.appendingPathComponent("backups", isDirectory: true)
      .appendingPathComponent(publicKey, isDirectory: true)
  }

  private func prepare(directory: String) throws {
    let access = try access(directory: directory)
    let marker = Data("tera.local.backup.v1\n".utf8)
    try access.installStagedBlob(marker, reference: RadrootsStagedBlobReference(blobID: "owner_v1", sizeBytes: marker.count))
    try Self.protect(accountDirectory)
    try Self.protect(access.roots.stagedBlobsRoot)
  }

  private func access(directory: String) throws -> RadrootsAppleFileAccess {
    try RadrootsAppleFileAccess(roots: RadrootsAppleFileRoots(
      appIdentifier: roots.appIdentifier, dataRoot: roots.dataRoot,
      cacheRoot: roots.cacheRoot, temporaryRoot: roots.temporaryRoot,
      stagedBlobsRoot: accountDirectory.appendingPathComponent(directory, isDirectory: true)
    ))
  }

  private func name(id: String, completed: Bool) -> String {
    "\(id)_\(completed ? "complete" : "candidate")"
  }
}
