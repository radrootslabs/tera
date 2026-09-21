import Foundation
@testable import TeraApp

actor NativeRecoveryScheduleTestStorage {
  private var value = TeraNativeRecoverySchedule(author: String(repeating: "a", count: 64), revision: 0, after: nil)

  func load() -> TeraNativeRecoverySchedule {
    value
  }

  func advance(expected: TeraNativeRecoverySchedule, after: String?) throws -> TeraNativeRecoverySchedule {
    guard expected == value else { throw TeraComposerAcknowledgment.unconfirmed }
    value = .init(author: value.author, revision: value.revision + (value.after == after ? 0 : 1), after: after)
    return value
  }
}

extension TeraScopeBackend {
  func nativeRecoverySchedule() async -> TeraNativeRecoverySchedule {
    await recoveryScheduleStorage.load()
  }

  func advanceNativeRecoverySchedule(expected: TeraNativeRecoverySchedule, after: String?) async throws -> TeraNativeRecoverySchedule {
    try await recoveryScheduleStorage.advance(expected: expected, after: after)
  }
}

extension AddBackend {
  func nativeRecoverySchedule() async -> TeraNativeRecoverySchedule {
    await submissionBackend.recoveryScheduleStorage.load()
  }

  func advanceNativeRecoverySchedule(expected: TeraNativeRecoverySchedule, after: String?) async throws -> TeraNativeRecoverySchedule {
    try await submissionBackend.recoveryScheduleStorage.advance(expected: expected, after: after)
  }
}
