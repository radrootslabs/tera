import Combine
import Foundation
import RadrootsKit

actor TeraBackgroundEventRouter {
  typealias Handler =
    @Sendable (
      _ identifier: String,
      _ completion: @escaping @Sendable () -> Void
    ) async -> Void

  static let shared = TeraBackgroundEventRouter()
  private static let maximumPendingEvents = 8
  private static let pendingTimeoutNanoseconds: UInt64 = 15_000_000_000

  private struct PendingEvent: Sendable {
    let token: UUID
    let identifier: String
    let completion: TeraCompletionOnce
    let timeout: Task<Void, Never>
  }

  private var expectedIdentifier: String?
  private var handler: Handler?
  private var pending: [PendingEvent] = []

  func attach(identifier: String, handler: @escaping Handler) async {
    expectedIdentifier = identifier
    self.handler = handler
    let events = pending
    pending.removeAll(keepingCapacity: true)
    for event in events {
      event.timeout.cancel()
      guard event.identifier == identifier else {
        event.completion.complete()
        continue
      }
      await handler(event.identifier) {
        event.completion.complete()
      }
    }
  }

  func handle(identifier: String, completion: TeraCompletionOnce) async {
    if let expectedIdentifier, identifier != expectedIdentifier {
      completion.complete()
      return
    }
    if let handler {
      await handler(identifier) {
        completion.complete()
      }
      return
    }

    while pending.count >= Self.maximumPendingEvents {
      let oldest = pending.removeFirst()
      oldest.timeout.cancel()
      oldest.completion.complete()
    }
    let token = UUID()
    let timeout = Task { [weak self] in
      do {
        try await Task.sleep(nanoseconds: Self.pendingTimeoutNanoseconds)
      } catch {
        return
      }
      await self?.expire(token: token)
    }
    pending.append(
      PendingEvent(
        token: token,
        identifier: identifier,
        completion: completion,
        timeout: timeout
      )
    )
  }

  func detachAndCompletePending() {
    handler = nil
    expectedIdentifier = nil
    let events = pending
    pending.removeAll()
    for event in events {
      event.timeout.cancel()
      event.completion.complete()
    }
  }

  private func expire(token: UUID) {
    guard let index = pending.firstIndex(where: { $0.token == token }) else { return }
    let event = pending.remove(at: index)
    event.completion.complete()
  }
}

struct TeraDiagnosticRecord: Codable, Sendable, Equatable {
  let name: String
  let category: String
  let level: String
  let fields: [String: String]
  let occurredAtUnixMilliseconds: Int64
}

actor TeraDiagnosticsBuffer: RadrootsTelemetry {
  private struct Entry: Sendable {
    let event: RadrootsTelemetryEvent
    let occurredAtUnixMilliseconds: Int64
  }

  private let capacity: Int
  private let policy = RadrootsTelemetryRedactionPolicy.default
  private var events: [Entry] = []

  init(capacity: Int = 128) {
    self.capacity = min(max(capacity, 16), 256)
  }

  func record(_ event: RadrootsTelemetryEvent) {
    guard let occurredAtUnixMilliseconds = try? TeraClock.signedUnixMilliseconds(
      from: event.occurredAt
    ) else {
      return
    }
    events.append(
      Entry(
        event: policy.redacted(event),
        occurredAtUnixMilliseconds: occurredAtUnixMilliseconds
      )
    )
    if events.count > capacity {
      events.removeFirst(events.count - capacity)
    }
  }

  func records() -> [TeraDiagnosticRecord] {
    events.map { entry in
      let event = entry.event
      return TeraDiagnosticRecord(
        name: event.name,
        category: event.category,
        level: event.level.rawValue,
        fields: Dictionary(
          uniqueKeysWithValues: event.fields.map { field in
            (field.key, field.value.renderedValue)
          }
        ),
        occurredAtUnixMilliseconds: entry.occurredAtUnixMilliseconds
      )
    }
  }
}

private struct TeraDiagnosticsDocument: Codable, Sendable {
  let schema: String
  let appVersion: String
  let appBuild: String
  let runtimeCrate: String
  let runtimeVersion: String
  let runtimePhase: String
  let relayProfile: String?
  let relayState: String?
  let relayCount: Int
  let records: [TeraDiagnosticRecord]
}

struct TeraProductionLifecycleServices: Sendable {
  let coordinator: TeraLifecycleCoordinator
  let backgroundTransfer: any RadrootsBackgroundTransfer
}

actor TeraLifecycleCoordinator {
  private static let maximumExportBytes = 256 * 1024

  private let telemetry: any RadrootsTelemetry
  private let buffer: TeraDiagnosticsBuffer
  private let fileAccess: RadrootsAppleFileAccess?
  private let transfer: (any RadrootsBackgroundTransfer)?
  private let transferIdentifier: String?
  private var backgroundEventsAttached = false

  init(
    telemetry: any RadrootsTelemetry,
    buffer: TeraDiagnosticsBuffer,
    fileAccess: RadrootsAppleFileAccess?,
    transfer: (any RadrootsBackgroundTransfer)?,
    transferIdentifier: String?
  ) {
    self.telemetry = telemetry
    self.buffer = buffer
    self.fileAccess = fileAccess
    self.transfer = transfer
    self.transferIdentifier = transferIdentifier
  }

  static func production(bundleIdentifier: String) throws -> TeraLifecycleCoordinator {
    try productionServices(bundleIdentifier: bundleIdentifier).coordinator
  }

  static func productionServices(
    bundleIdentifier: String
  ) throws -> TeraProductionLifecycleServices {
    let roots = try TeraRemoteQualificationEnvironment.applicationFileRoots(
      appIdentifier: bundleIdentifier
    )
    let fileAccess = RadrootsAppleFileAccess(roots: roots)
    let buffer = TeraDiagnosticsBuffer()
    let logger = RadrootsAppleLoggerTelemetry(subsystem: bundleIdentifier)
    let telemetry = RadrootsMultiplexTelemetry([
      logger,
      RadrootsRedactingTelemetry(sink: buffer),
    ])
    let identifier = try RadrootsBackgroundTransferValidation.normalizedIdentifier(
      TeraRemoteQualificationEnvironment.backgroundTransferIdentifier(
        appIdentifier: bundleIdentifier
      )
    )
    let transfer = try RadrootsAppleBackgroundTransfer(
      roots: roots,
      sessionIdentifier: identifier
    )
    let coordinator = TeraLifecycleCoordinator(
      telemetry: telemetry,
      buffer: buffer,
      fileAccess: fileAccess,
      transfer: transfer,
      transferIdentifier: identifier
    )
    return TeraProductionLifecycleServices(
      coordinator: coordinator,
      backgroundTransfer: transfer
    )
  }

  static func disabled() -> TeraLifecycleCoordinator {
    let buffer = TeraDiagnosticsBuffer()
    return TeraLifecycleCoordinator(
      telemetry: RadrootsRedactingTelemetry(sink: buffer),
      buffer: buffer,
      fileAccess: nil,
      transfer: nil,
      transferIdentifier: nil
    )
  }

  static func testing(roots: RadrootsAppleFileRoots, capacity: Int = 128)
    -> TeraLifecycleCoordinator
  {
    let buffer = TeraDiagnosticsBuffer(capacity: capacity)
    return TeraLifecycleCoordinator(
      telemetry: RadrootsRedactingTelemetry(sink: buffer),
      buffer: buffer,
      fileAccess: RadrootsAppleFileAccess(roots: roots),
      transfer: nil,
      transferIdentifier: nil
    )
  }

  func attachBackgroundEvents() async {
    guard !backgroundEventsAttached,
      let transfer,
      let transferIdentifier
    else {
      return
    }
    backgroundEventsAttached = true
    await TeraBackgroundEventRouter.shared.attach(identifier: transferIdentifier) {
      identifier,
      completion in
      await transfer.handleEventsForBackgroundURLSession(
        identifier: identifier,
        completionHandler: completion
      )
    }
  }

  func record(
    _ name: String,
    level: RadrootsTelemetryLevel = .info,
    fields: [String: String] = [:]
  ) async {
    let values = fields.sorted(by: { $0.key < $1.key }).compactMap { key, value in
      try? RadrootsTelemetryField.string(key, value)
    }
    guard
      let event = try? RadrootsTelemetryEvent(
        name: name,
        category: "ios_lifecycle",
        level: level,
        fields: values
      )
    else {
      return
    }
    await telemetry.record(event)
  }

  func prepareDiagnostics(
    snapshot: TeraRuntimeSnapshot,
    appVersion: String,
    appBuild: String,
    phase: String
  ) async throws -> RadrootsPreparedExportDocument {
    guard let fileAccess else {
      throw TeraRuntimeFailure.local(
        operation: "diagnostics.prepare",
        code: "ios.diagnostics.unavailable",
        safeMessage: "Diagnostics export is unavailable."
      )
    }
    let records = await buffer.records()
    let document = TeraDiagnosticsDocument(
      schema: "radroots.ios.diagnostics.v1",
      appVersion: appVersion,
      appBuild: appBuild,
      runtimeCrate: snapshot.crateName,
      runtimeVersion: snapshot.crateVersion,
      runtimePhase: phase,
      relayProfile: snapshot.relay?.profile,
      relayState: snapshot.relay?.state,
      relayCount: snapshot.relay?.relays.count ?? 0,
      records: records
    )
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    let data = try encoder.encode(document)
    guard data.count <= Self.maximumExportBytes else {
      throw TeraRuntimeFailure.local(
        operation: "diagnostics.prepare",
        code: "ios.diagnostics.too_large",
        safeMessage: "The bounded diagnostics report could not be prepared."
      )
    }
    return try fileAccess.prepareExport(
      RadrootsExportDocumentRequest(
        source: .inlineData(data),
        suggestedFilename: "tera-diagnostics.json",
        mediaType: "application/json",
        sizeBytes: UInt64(data.count)
      )
    )
  }

  func releaseDiagnostics(_ export: RadrootsPreparedExportDocument) {
    guard let fileAccess else { return }
    try? fileAccess.releasePreparedExport(export)
  }
}

@MainActor
final class TeraDiagnosticsStore: ObservableObject {
  @Published var preparedExport: RadrootsPreparedExportDocument?
  @Published private(set) var isPreparing = false
  @Published private(set) var message: String?

  private let coordinator: TeraLifecycleCoordinator
  private var activeExport: RadrootsPreparedExportDocument?

  init(coordinator: TeraLifecycleCoordinator) {
    self.coordinator = coordinator
  }

  func prepare(snapshot: TeraRuntimeSnapshot, bundle: Bundle = .main) async {
    guard !isPreparing, preparedExport == nil, activeExport == nil else { return }
    isPreparing = true
    message = nil
    defer { isPreparing = false }
    do {
      let export = try await coordinator.prepareDiagnostics(
        snapshot: snapshot,
        appVersion: bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
          ?? "0",
        appBuild: bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "0",
        phase: "running"
      )
      activeExport = export
      preparedExport = export
    } catch {
      activeExport = nil
      preparedExport = nil
      message = TeraUserMessages.text(.diagnosticsPrepareFailed)
    }
  }

  func completeExport(_ result: Result<RadrootsExportDocumentResult, Error>) {
    let export = activeExport
    activeExport = nil
    preparedExport = nil
    switch result {
    case .success:
      message = TeraUserMessages.text(.diagnosticsExportSucceeded)
    case .failure:
      message = TeraUserMessages.text(.diagnosticsExportFailed)
    }
    guard let export else { return }
    Task { await coordinator.releaseDiagnostics(export) }
  }
}
