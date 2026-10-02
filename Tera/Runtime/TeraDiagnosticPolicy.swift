import Foundation
import RadrootsKit

/// An app-owned closed schema, applied before all sinks and again at the
/// standalone buffer boundary. Pattern redaction alone cannot admit content.
enum TeraDiagnosticPolicy {
  static let eventNames: Set<String> = [
    "ios.lifecycle.start_requested", "ios.lifecycle.resume_coalesced",
    "ios.lifecycle.active", "ios.lifecycle.background",
    "ios.lifecycle.protected_data_available", "ios.lifecycle.protected_data_unavailable",
    "ios.lifecycle.operation_failed", "ios.lifecycle.operation_completed",
    "ios.lifecycle.shutdown_requested", "ios.diagnostics.redacted",
  ]
  static let phases: Set<String> = [
    "starting", "identity_required", "identity_locked", "protected_data_unavailable",
    "recovery_required", "corrupt_identity", "configuration_reconfiguration_required",
    "running", "stopped", "failed",
  ]
  private static let operations: Set<String> = [
    "start", "stop", "identity_create", "identity_import", "identity_lock", "identity_unlock",
    "identity_recover", "configuration_reconfigure", "settings_reconfigure", "identity_key_removal",
  ]

  static func sanitized(_ event: RadrootsTelemetryEvent) -> RadrootsTelemetryEvent? {
    let known = eventNames.contains(event.name)
    let name = known ? event.name : "ios.diagnostics.redacted"
    let fields = known ? event.fields.prefix(8).compactMap(sanitizedField) : []
    return try? RadrootsTelemetryEvent(
      name: name, category: "ios_lifecycle", level: event.level,
      fields: fields, occurredAt: event.occurredAt
    )
  }

  private static func sanitizedField(_ field: RadrootsTelemetryField) -> RadrootsTelemetryField? {
    if case let .string(value) = field.value {
      switch field.key {
      case "operation" where operations.contains(value): return field
      case "phase" where phases.contains(value): return field
      case "code" where value == "bootstrap_failed": return field
      default: return nil
      }
    }
    if case let .integer(value) = field.value,
      ["count", "attempts", "schema", "latency_ms"].contains(field.key),
      (0 ... 1_000_000).contains(value)
    {
      return field
    }
    return nil
  }

  static func code(_ value: String, allowed: Set<String>) -> String {
    allowed.contains(value) ? value : "unavailable"
  }

  static func version(_ value: String) -> String {
    code(value, allowed: ["0.1.0-alpha"])
  }

  static func build(_ value: String) -> String {
    guard !value.isEmpty, value.utf8.count <= 10,
      value.utf8.allSatisfy({ (48 ... 57).contains($0) })
    else { return "unavailable" }
    return value
  }
}

struct TeraSanitizedTelemetry: RadrootsTelemetry {
  let sink: any RadrootsTelemetry

  func record(_ event: RadrootsTelemetryEvent) async {
    guard let event = TeraDiagnosticPolicy.sanitized(event) else { return }
    await sink.record(event)
  }
}
