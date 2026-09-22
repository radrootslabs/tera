import Foundation

enum TeraRestorePhase: Sendable, Equatable { case held, reviewed, resumed }
enum TeraRestoreObservation: Sendable, Equatable { case observed, notObserved, incomplete }

struct TeraRestoreTarget: Sendable, Equatable {
  let draftID: String
  let eventID: String
  let targetFingerprint: String
  let observation: TeraRestoreObservation?
}

struct TeraRestoreStatus: Sendable, Equatable {
  let attemptID: String
  let phase: TeraRestorePhase
  let targets: [TeraRestoreTarget]
}

extension TeraRuntimeClient {
  func restoreStatus() async throws -> TeraRestoreStatus? {
    try await addOperation("runtime.restore.status") { try await $0.restoreStatus() }
  }

  func reconcileRestoredTarget(_ target: TeraRestoreTarget) async throws {
    try await addOperation("runtime.restore.reconcile") { try await $0.reconcileRestoredTarget(target) }
  }

  func reviewRestoredWork() async throws -> String {
    try await addOperation("runtime.restore.review") { try await $0.reviewRestoredWork() }
  }

  func resumeRestoredWork(reviewedInventory: String) async throws {
    try await addOperation("runtime.restore.resume") { try await $0.resumeRestoredWork(reviewedInventory: reviewedInventory) }
  }
}

extension TeraRuntimeBackend {
  func restoreStatus() async throws -> TeraRestoreStatus? {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func reconcileRestoredTarget(_: TeraRestoreTarget) async throws {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func reviewRestoredWork() async throws -> String {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func resumeRestoredWork(reviewedInventory _: String) async throws {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}
