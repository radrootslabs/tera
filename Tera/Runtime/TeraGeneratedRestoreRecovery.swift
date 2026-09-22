import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  func restoreStatus() async throws -> TeraRestoreStatus? {
    do {
      guard let value = try await runtime.applicationRestoreStatus() else { return nil }
      let phase: TeraRestorePhase = switch value.phase {
      case .held: .held
      case .reviewed: .reviewed
      case .resumed: .resumed
      }
      return TeraRestoreStatus(attemptID: value.attemptId, phase: phase, targets: value.targets.map(Self.restoreTarget))
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func reconcileRestoredTarget(_ target: TeraRestoreTarget) async throws {
    do {
      let value = try await runtime.reconcileRestoredTarget(draftId: target.draftID, targetFingerprint: target.targetFingerprint)
      guard value.draftId == target.draftID, value.eventId == target.eventID,
        value.targetFingerprint == target.targetFingerprint else { throw TeraComposerAcknowledgment.unconfirmed }
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func reviewRestoredWork() async throws -> String {
    do { return try await runtime.reviewRestoredWork() } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func resumeRestoredWork(reviewedInventory: String) async throws {
    do { try await runtime.resumeRestoredWork(reviewedInventory: reviewedInventory) } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  private static func restoreTarget(_ value: FfiRestoreTarget) -> TeraRestoreTarget {
    let observation: TeraRestoreObservation? = value.observation.map {
      switch $0 {
      case .observed: .observed
      case .notObserved: .notObserved
      case .incomplete: .incomplete
      }
    }
    return TeraRestoreTarget(draftID: value.draftId, eventID: value.eventId,
                             targetFingerprint: value.targetFingerprint, observation: observation)
  }
}
