import CryptoKit
import Foundation
import RadrootsKit

enum TeraNativeRecoveryReason: Sendable, Equatable {
  case missingParent, invalidParent, associationMismatch, outcomeUnconfirmed, resolved
}

enum TeraNativeRecoveryPause: Sendable, Equatable {
  case protectedData, storageUnavailable
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
    if let failure = TeraRuntimeFailure.from(error) {
      switch failure.recovery.disposition {
      case .protectedDataUnavailable: return .protectedData
      case .storageFailure, .quotaExhausted, .runtimeUnavailable, .identityUnavailable: return .storageUnavailable
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
    let status = try? await client.reportNativeRecoveryStatus(key: key, reason: reason)
    guard reason != .resolved else { return nil }
    return .init(key: key, reason: reason, status: status)
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
