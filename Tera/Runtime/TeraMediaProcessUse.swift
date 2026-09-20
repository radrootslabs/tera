import Darwin
import Foundation
import RadrootsKit

/// Records admission before underlying work can outlive a caller or runtime.
/// The marker deliberately survives shutdown, cancellation and failed creation.
/// It is local coordination evidence, never a portable ownership database.
enum TeraMediaProcessUse {
  static let directory = ".tera_media_use"

  static func admit(applicationSupportDirectory: String) throws -> RadrootsFileUseReservation {
    let parts = applicationSupportDirectory.split(separator: "/")
    guard applicationSupportDirectory.hasPrefix("/"), !applicationSupportDirectory.utf8.contains(0),
      !parts.isEmpty, parts.allSatisfy({ $0 != "." && $0 != ".." })
    else { throw RadrootsAppleFileError.invalidRequest }
    return try admit(root: URL(fileURLWithPath: applicationSupportDirectory, isDirectory: true))
  }

  static func admit(root: URL) throws -> RadrootsFileUseReservation {
    guard let use = try RadrootsAppleFileMaintenance(root: root).reserveUse() else {
      throw RadrootsAppleFileError.transientFailure
    }
    let roots = try RadrootsAppleFileRoots(
      appIdentifier: "tera.media.use", dataRoot: root, cacheRoot: root, temporaryRoot: root
    )
    let pid = getpid()
    let file = RadrootsFileReference(scope: .data, relativePath: "\(directory)/\(pid)")
    try RadrootsAppleFileAccess(roots: roots).write(.inline(Data("tera.media.use.v1 \(pid)\n".utf8)), to: file)
    try use.validate()
    return use
  }

  /// Must be called under the exclusive reservation. ESRCH is the only admitted
  /// absence observation. PID reuse and permission failures conservatively retain.
  static func hasNoLiveUsers(
    roots: RadrootsAppleFileRoots,
    scan: RadrootsFileMaintenanceScan,
    remaining: inout Int,
    isAbsent: (Int32) -> Bool = definitelyAbsent
  ) throws -> Bool {
    let access = RadrootsAppleFileAccess(roots: roots)
    while remaining > 0 {
      let page = try scan.next(limit: min(64, remaining))
      remaining -= page.scannedEntries
      for entry in page.entries {
        guard entry.kind == .regularFile, entry.sizeBytes > 0, entry.sizeBytes <= 64,
          let pid = Int32(entry.name), pid > 0, String(pid) == entry.name
        else { return false }
        let file = RadrootsFileReference(scope: .data, relativePath: "\(directory)/\(entry.name)")
        guard case let .inline(data) = try access.read(file, mode: .inline(maxBytes: 64)),
          data == Data("tera.media.use.v1 \(pid)\n".utf8), isAbsent(pid)
        else { return false }
        // Never reuse a marker after a conditional-unlink identity mismatch.
        guard try scan.remove(entry) else { return false }
      }
      if page.reachedEnd {
        return true
      }
    }
    return false
  }

  private static func definitelyAbsent(_ pid: Int32) -> Bool {
    let result = Darwin.kill(pid, 0)
    return result == -1 && errno == ESRCH
  }
}
