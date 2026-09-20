import Foundation
import RadrootsKit
import TeraKitBindings
import UIKit

enum TeraMediaCleanupResult: Sendable, Equatable {
  case retained
  case completed(removed: Int, exhaustedBudget: Bool)
}

/// An explicit startup pass. It never starts an OS session, signs, publishes,
/// walks export/lease trees, or deletes a file by a reconstructed pathname.
enum TeraMediaCleanup {
  static func run(
    roots: RadrootsAppleFileRoots, now: Date = Date(),
    protectedData: @Sendable () async -> Bool = protectedDataAvailable
  ) async -> TeraMediaCleanupResult {
    guard await protectedData(), !Task.isCancelled else { return .retained }
    do {
      guard let reservation = try RadrootsAppleFileMaintenance(root: roots.dataRoot).reserveMaintenance() else { return .retained }
      defer { withExtendedLifetime(reservation) {} }
      let limits = mediaCleanupLimits()
      guard var remaining = Int(exactly: limits.directoryEntries), let removals = Int(exactly: limits.removals) else { return .retained }
      let children = try rootChildren(reservation: reservation, remaining: &remaining)
      if let users = children[TeraMediaProcessUse.directory] {
        guard users.kind == .directory,
          try TeraMediaProcessUse.hasNoLiveUsers(
            roots: roots, scan: reservation.openDirectory(relativePath: TeraMediaProcessUse.directory), remaining: &remaining
          )
        else { return .retained }
      }
      // Future backup consumers must join the admission protocol before this
      // conservative exclusion can be relaxed by their owning checkpoint.
      guard children["backups"] == nil else { return .retained }
      guard let staged = children["staged_blobs"] else { return .completed(removed: 0, exhaustedBudget: false) }
      guard staged.kind == .directory,
        roots.stagedBlobsRoot.standardizedFileURL == roots.dataRoot.appendingPathComponent("staged_blobs", isDirectory: true).standardizedFileURL
      else { return .retained }
      let snapshots = try await RadrootsAppleBackgroundTransferStore(roots: roots).loadSnapshots()
      let native = try nativeHashes(snapshots, limit: limits.nativeReferences)
      let inventory = try await inspectMediaReferences(applicationSupportDirectory: roots.dataRoot.path, nativeHashes: native)
      guard await protectedData() else { return .retained }
      try Task.checkCancellation()
      try reservation.validate()
      let scan = try reservation.openDirectory(relativePath: "staged_blobs")
      return try collect(scan: scan, inventory: inventory, now: now, remaining: remaining, removals: removals)
    } catch {
      // Includes cancellation and an ambiguous unlink. No old proof is reused.
      return .retained
    }
  }

  private static func rootChildren(
    reservation: RadrootsFileMaintenanceReservation, remaining: inout Int
  ) throws -> [String: RadrootsFileMaintenanceEntry] {
    let scan = try reservation.openDirectory()
    var children: [String: RadrootsFileMaintenanceEntry] = [:]
    while remaining > 0 {
      let page = try scan.next(limit: min(64, remaining))
      remaining -= page.scannedEntries
      for entry in page.entries where [TeraMediaProcessUse.directory, "staged_blobs", "backups"].contains(entry.name) {
        children[entry.name] = entry
      }
      if page.reachedEnd {
        return children
      }
    }
    throw RadrootsAppleFileError.transientFailure
  }

  static func nativeHashes(_ snapshots: [RadrootsBackgroundTransferSnapshot], limit: UInt64) throws -> [String] {
    guard UInt64(snapshots.count) <= limit / 3 else { throw RadrootsAppleFileError.invalidRequest }
    var hashes: [String] = []
    for snapshot in snapshots {
      // Preserve completed history too: a native status is not a Rust receipt.
      guard let expected = snapshot.request.expectedSourceSHA256 else { throw RadrootsAppleFileError.invalidRequest }
      hashes.append(expected)
      let source: RadrootsBackgroundTransferLocalFile = switch snapshot.request.operation {
      case let .upload(value): value
      case let .download(value): value
      }
      switch source {
      case let .stagedBlob(blob): hashes.append(blob.blobID)
      case let .file(file):
        if file.scope == .data, file.relativePath.hasPrefix("staged_blobs/") {
          hashes.append(String(file.relativePath.dropFirst("staged_blobs/".count)))
        }
      }
      if let lease = snapshot.uploadLease {
        hashes.append(lease.blobID)
      }
    }
    return hashes
  }

  private static func collect(
    scan: RadrootsFileMaintenanceScan, inventory: FfiMediaReferenceInventory,
    now: Date, remaining: Int, removals: Int
  ) throws -> TeraMediaCleanupResult {
    guard let nowMS = milliseconds(now, rounding: .down) else { return .retained }
    var remaining = remaining
    var removed = 0
    while remaining > 0, removed < removals {
      try Task.checkCancellation()
      let page = try scan.next(limit: min(64, remaining))
      remaining -= page.scannedEntries
      for entry in page.entries {
        guard removed < removals else { return .completed(removed: removed, exhaustedBudget: true) }
        guard entry.kind == .regularFile, let modified = milliseconds(entry.modifiedAt, rounding: .up),
          inventory.permitsOrphan(name: entry.name, modifiedMs: modified, nowMs: nowMS)
        else { continue }
        if try scan.remove(entry) {
          removed += 1
        }
      }
      if page.reachedEnd {
        return .completed(removed: removed, exhaustedBudget: false)
      }
    }
    return .completed(removed: removed, exhaustedBudget: true)
  }

  private static func milliseconds(_ date: Date, rounding: FloatingPointRoundingRule) -> UInt64? {
    let value = date.timeIntervalSince1970 * 1000
    guard value.isFinite, value > 0, value < Double(Int64.max) else { return nil }
    return UInt64(value.rounded(rounding))
  }

  private static func protectedDataAvailable() async -> Bool {
    await MainActor.run { UIApplication.shared.isProtectedDataAvailable }
  }
}
