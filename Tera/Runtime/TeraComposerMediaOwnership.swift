import CryptoKit
import Foundation
import RadrootsKit

/// The Apple host proves file durability; Rust remains the sole owner of scoped
/// composer references. The two stores deliberately have no shared transaction.
enum TeraComposerMediaOwnership {
  static func confirm(_ media: [TeraComposerMedia], roots: RadrootsAppleFileRoots) throws {
    guard !media.isEmpty else { return }
    let durable = try TeraDurableMediaRoots.selectingStaging(in: roots)
    guard media.count <= 20, roots.stagedBlobsRoot == durable.stagedBlobsRoot else {
      throw TeraComposerAcknowledgment.unconfirmed
    }
    let access = RadrootsAppleFileAccess(roots: roots)
    for item in media {
      try Task.checkCancellation()
      let blob = try reference(item)
      try TeraDurableMediaRoots.restoreLegacyBlob(blob, roots: roots)
      // Governed reads reject symlinks and oversized/replaced files. Process one
      // derivative at a time under the preparer's existing 10 MiB output bound.
      let bytes = try access.readStagedBlob(blob)
      let digest = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
      guard digest == item.sha256 else { throw TeraComposerAcknowledgment.unconfirmed }
      // This exact install flushes both file and directory, including an
      // existing matching object. It never deletes another draft's reference.
      try access.installStagedBlob(bytes, reference: blob)
    }
    // Hash-named published files are identifiable orphans if the following DB
    // commit fails or its result is lost. Keep them for C087 reconciliation.
  }

  private static func reference(_ item: TeraComposerMedia) throws -> RadrootsStagedBlobReference {
    guard item.sha256.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil,
          item.opaqueReference == "media:\(item.sha256)", item.mediaType == "image/png",
          (1 ... 10 * 1024 * 1024).contains(item.byteSize), let size = Int(exactly: item.byteSize)
    else { throw TeraComposerAcknowledgment.unconfirmed }
    return try RadrootsStagedBlobReference(blobID: item.sha256, sizeBytes: size,
                                           mediaType: item.mediaType, filenameHint: "\(item.sha256).png")
  }
}
