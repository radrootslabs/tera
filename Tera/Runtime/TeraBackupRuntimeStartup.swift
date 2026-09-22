import Foundation
import RadrootsKit
import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  /// Call instead of ordinary startup, after the session has closed its prior
  /// runtime. The existing single database owner and file paths remain intact.
  static func startWithLocalBackups(configuration: TeraRuntimeLaunchConfiguration) async throws -> TeraRuntimeBackendStart {
    guard configuration.protectedData == .available else { throw TeraLocalBackupHost.failure("backup_unavailable") }
    let use = try TeraMediaProcessUse.admit(applicationSupportDirectory: configuration.applicationSupportDirectory)
    defer { withExtendedLifetime(use) {} }
    let root = URL(fileURLWithPath: configuration.applicationSupportDirectory, isDirectory: true)
    let roots = try RadrootsAppleFileRoots(appIdentifier: configuration.app.bundleIdentifier,
                                           dataRoot: root, cacheRoot: root, temporaryRoot: root)
    let host = try TeraLocalBackupHost(roots: roots, publicKey: configuration.publicKeyHex,
                                       generation: configuration.sourceGenerationHex)
    // Structural binding validation for directory preparation; this creates no
    // backup candidate and no publication or retained request identity.
    try await host.prepare(request: FfiBackupRequest(schemaVersion: applicationBackupLimits().schemaVersion,
                                                     backupId: String(repeating: "01", count: 16), publicKey: configuration.publicKeyHex,
                                                     sourceGeneration: configuration.sourceGenerationHex, requestedAtUnixMs: configuration.sourceGenerationCreatedAtUnixMilliseconds,
                                                     maximumBytes: 1))
    return try await start(configuration: configuration, localBackups: true)
  }
}
