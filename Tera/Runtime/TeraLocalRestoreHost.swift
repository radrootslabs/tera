import Foundation
import RadrootsKit
import TeraKitBindings
import UIKit

/// Explicit cold recovery only. It must run before ordinary runtime/transfer
/// admission in a fresh process. Live work is preserved and makes recovery busy.
actor TeraLocalRestoreHost: TeraRestoreHost {
  let roots: RadrootsAppleFileRoots
  let activeTransfers: @Sendable () async throws -> Set<RadrootsBackgroundTransferIdentifier>
  let protectedData: @Sendable () async -> Bool
  private var reservation: RadrootsFileMaintenanceReservation?
  private var selected: FfiRestoreRequest?

  init(roots: RadrootsAppleFileRoots,
       activeTransfers: @escaping @Sendable () async throws -> Set<RadrootsBackgroundTransferIdentifier>,
       protectedData: @escaping @Sendable () async -> Bool = currentProtectedData) throws
  {
    self.roots = try TeraDurableMediaRoots.selectingStaging(in: roots)
    self.activeTransfers = activeTransfers
    self.protectedData = protectedData
  }

  static func production(roots: RadrootsAppleFileRoots) throws -> TeraLocalRestoreHost {
    let identifier = try RadrootsBackgroundTransferValidation.normalizedIdentifier(
      TeraRemoteQualificationEnvironment.backgroundTransferIdentifier(appIdentifier: roots.appIdentifier)
    )
    let store = RadrootsAppleBackgroundTransferStore(roots: roots)
    let resolver = RadrootsAppleBackgroundTransferFileResolver(roots: roots)
    let downloads = try roots.resolvedURL(for: RadrootsFileReference(
      scope: .temporary, relativePath: "background_transfers/\(identifier)/downloads"
    ), allowRootDirectory: true)
    let adapters = try RadrootsAppleBackgroundTransferAdapters.live(
      sessionIdentifier: identifier, store: store, fileResolver: resolver, downloadStagingRoot: downloads
    )
    return try Self(roots: roots, activeTransfers: adapters.activeTransferIdentifiers)
  }

  func restore(store: FfiRestoreStore, request: FfiRestoreRequest) async throws -> FfiRestoreGuard {
    guard reservation == nil, selected == nil,
      store.applicationSupportDirectory == roots.dataRoot.path,
      store.publicKey == request.backup.publicKey, store.sourceGeneration == request.backup.sourceGeneration,
      let admission = try RadrootsAppleFileMaintenance(root: roots.dataRoot).reserveMaintenance()
    else { throw Self.failure("restore_busy") }
    reservation = admission
    selected = request
    defer { reservation = nil; selected = nil; withExtendedLifetime(admission) {} }
    try requireNoLiveUsers(admission)
    return try await restoreLocalApplicationBackup(store: store, request: request, host: self)
  }

  func requireQuiescent() async throws {
    guard let reservation, selected != nil, await protectedData(), !Task.isCancelled else {
      throw Self.failure("restore_recovery_required")
    }
    do {
      try reservation.validate()
      guard try await activeTransfers().isEmpty else { throw Self.failure("restore_busy") }
      try Task.checkCancellation()
      try reservation.validate()
    } catch { throw Self.bound(error) }
  }

  func loadCompleted(request: FfiRestoreRequest) async throws -> Data {
    guard selected == request else { throw Self.failure("restore_conflict") }
    try await requireQuiescent()
    do {
      guard let bytes = try TeraBackupFiles(roots: roots, publicKey: request.backup.publicKey).read(id: request.backup.backupId, completed: true) else {
        throw Self.failure("restore_recovery_required")
      }
      return bytes
    } catch { throw Self.bound(error) }
  }

  func restoreMedia(manifest: FfiBackupManifest) async throws {
    guard let selected, selected.backup == manifest.request else { throw Self.failure("restore_conflict") }
    try await requireQuiescent()
    do {
      try TeraRestoreFiles(roots: roots, publicKey: selected.backup.publicKey).restoreMedia(manifest)
    } catch { throw Self.bound(error) }
  }

  func armGuard(guard guardRecord: FfiRestoreGuard) async throws {
    guard selected == guardRecord.request else { throw Self.failure("restore_conflict") }
    try await requireQuiescent()
    do {
      try TeraRestoreFiles(roots: roots, publicKey: guardRecord.request.backup.publicKey).arm(guardRecord)
    } catch { throw Self.bound(error) }
  }

  private func requireNoLiveUsers(_ admission: RadrootsFileMaintenanceReservation) throws {
    let scan = try admission.openDirectory()
    var remaining = Int(mediaCleanupLimits().directoryEntries)
    while remaining > 0 {
      let page = try scan.next(limit: min(64, remaining))
      remaining -= page.scannedEntries
      if let users = page.entries.first(where: { $0.name == TeraMediaProcessUse.directory }) {
        guard users.kind == .directory,
          try TeraMediaProcessUse.hasNoLiveUsers(roots: roots, scan: admission.openDirectory(relativePath: TeraMediaProcessUse.directory), remaining: &remaining)
        else { throw Self.failure("restore_busy") }
      }
      if page.reachedEnd {
        return
      }
    }
    throw Self.failure("restore_capacity_exceeded")
  }

  nonisolated static func failure(_ code: String) -> TeraAppError {
    .Failure(report: TeraErrorRecord(schemaVersion: 1, code: code, category: "restore", retryable: false,
                                     recoveryActions: ["review_restore"], operationId: nil, capabilityId: nil,
                                     safeMessage: "Local recovery requires review before continuing."))
  }

  private nonisolated static func bound(_ error: Error) -> TeraAppError {
    error as? TeraAppError ?? failure("restore_recovery_required")
  }

  private static func currentProtectedData() async -> Bool {
    await MainActor.run { UIApplication.shared.isProtectedDataAvailable }
  }
}
