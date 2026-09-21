import CryptoKit
import Foundation
import RadrootsKit

enum TeraNativeRecoveryReason: Sendable, Equatable {
  case missingParent, invalidParent, associationMismatch, outcomeUnconfirmed, resolved
}

enum TeraNativeRecoveryPause: Sendable, Equatable {
  case protectedData, storageUnavailable, quota, credentials, runtimeUnavailable

  var message: String {
    let reason = switch self {
    case .protectedData: "Photo recovery is paused until this device is unlocked."
    case .storageUnavailable: "Photo recovery is paused until local storage is available."
    case .quota: "Photo recovery is paused because local storage is full. Free space, then check again."
    case .credentials: "Photo recovery is paused until this account’s credentials are available."
    case .runtimeUnavailable: "Photo recovery is paused until this account’s runtime is available."
    }
    return "\(reason) Saved editing is still available."
  }
}

enum TeraNativeRecoveryFault: Error {
  case missingParent, invalidParent, associationMismatch
}

struct TeraNativeRecoveryStatus: Sendable, Equatable {
  let key: String
  let reason: TeraNativeRecoveryReason
  let revision: UInt64
  let firstObservedUnixMS: UInt64
  let updatedAtUnixMS: UInt64
}

struct TeraNativeRecoveryIssue: Sendable, Equatable, Identifiable {
  let key: String
  let reason: TeraNativeRecoveryReason
  let status: TeraNativeRecoveryStatus?
  var id: String {
    key
  }

  static func key(_ identifier: String) -> String {
    SHA256.hash(data: Data(identifier.utf8)).map { String(format: "%02x", $0) }.joined()
  }
}

enum TeraNativeRecoveryClassification {
  static func pause(_ error: Error) -> TeraNativeRecoveryPause? {
    if let client = error as? TeraRuntimeClientError, case .notRunning = client {
      return .runtimeUnavailable
    }
    if let failure = TeraRuntimeFailure.from(error) {
      switch failure.recovery.disposition {
      case .protectedDataUnavailable: return .protectedData
      case .storageFailure: return .storageUnavailable
      case .quotaExhausted: return .quota
      case .identityUnavailable: return .credentials
      case .runtimeUnavailable: return .runtimeUnavailable
      default: break
      }
    }
    if let native = error as? RadrootsBackgroundTransferError, native == .persistenceFailure || native == .unavailable {
      return .storageUnavailable
    }
    return nil
  }

  static func reason(_ error: Error) -> TeraNativeRecoveryReason {
    switch error {
    case TeraNativeRecoveryFault.missingParent: .missingParent
    case TeraNativeRecoveryFault.invalidParent: .invalidParent
    case TeraNativeRecoveryFault.associationMismatch: .associationMismatch
    default: .outcomeUnconfirmed
    }
  }

  static func report(_ identifier: String, reason: TeraNativeRecoveryReason,
                     client: TeraRuntimeClient) async -> TeraNativeRecoveryIssue?
  {
    let key = TeraNativeRecoveryIssue.key(identifier)
    do {
      let status = try await client.reportNativeRecoveryStatus(key: key, reason: reason)
      return .init(key: key, reason: reason, status: status)
    } catch {
      guard reason == .resolved else { return .init(key: key, reason: reason, status: nil) }
      // A new store may have no transient notice. Read back a retained advisory
      // after a failed resolution write; only durable resolved state clears it.
      do {
        return try await client.nativeRecoveryStatus(key: key).map { .init(key: key, reason: $0.reason, status: $0) }
      } catch {
        return .init(key: key, reason: .outcomeUnconfirmed, status: nil)
      }
    }
  }
}

extension TeraRuntimeBackend {
  func nativeRecoveryStatus(key _: String) async throws -> TeraNativeRecoveryStatus? {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func reportNativeRecoveryStatus(key _: String, reason _: TeraNativeRecoveryReason) async throws -> TeraNativeRecoveryStatus? {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}

extension TeraRuntimeClient {
  func nativeRecoveryStatus(key: String) async throws -> TeraNativeRecoveryStatus? {
    try await addOperation("runtime.recovery.status") { try await $0.nativeRecoveryStatus(key: key) }
  }

  func reportNativeRecoveryStatus(key: String, reason: TeraNativeRecoveryReason) async throws -> TeraNativeRecoveryStatus? {
    try await addOperation("runtime.recovery.report") { try await $0.reportNativeRecoveryStatus(key: key, reason: reason) }
  }
}
