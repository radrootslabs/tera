import Foundation
import TeraKitBindings

struct TeraNativeRecoverySchedule: Sendable, Equatable {
  let author: String
  let revision: UInt64
  let after: String?
}

extension TeraRuntimeBackend {
  func nativeRecoverySchedule() async throws -> TeraNativeRecoverySchedule {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func advanceNativeRecoverySchedule(expected _: TeraNativeRecoverySchedule, after _: String?) async throws -> TeraNativeRecoverySchedule {
    throw TeraComposerAcknowledgment.unconfirmed
  }
}

extension TeraRuntimeClient {
  func nativeRecoverySchedule() async throws -> TeraNativeRecoverySchedule {
    try await addOperation("runtime.recovery.schedule") { try await $0.nativeRecoverySchedule() }
  }

  func advanceNativeRecoverySchedule(expected: TeraNativeRecoverySchedule, after: String?) async throws -> TeraNativeRecoverySchedule {
    try await addOperation("runtime.recovery.schedule.advance") { try await $0.advanceNativeRecoverySchedule(expected: expected, after: after) }
  }
}

extension TeraGeneratedRuntimeBackend {
  func nativeRecoverySchedule() async throws -> TeraNativeRecoverySchedule {
    do {
      return try await runtime.nativeRecoverySchedule(schemaVersion: 1).appValue()
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func advanceNativeRecoverySchedule(expected: TeraNativeRecoverySchedule, after: String?) async throws -> TeraNativeRecoverySchedule {
    do {
      let value = try await runtime.advanceNativeRecoverySchedule(expected: .init(schemaVersion: 1, author: expected.author,
                                                                                  revision: expected.revision, after: expected.after), after: after).appValue()
      guard value.author == expected.author, value.after == after,
            value.revision == expected.revision + (expected.after == after ? 0 : 1)
      else { throw TeraComposerAcknowledgment.unconfirmed }
      return value
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }
}

private extension FfiNativeRecoverySchedule {
  func appValue() throws -> TeraNativeRecoverySchedule {
    let canonical = { (value: String) in value.utf8.count == 64 && value.utf8.allSatisfy { (48 ... 57).contains($0) || (97 ... 102).contains($0) } }
    guard schemaVersion == 1, canonical(author), revision <= UInt64(Int64.max),
          after.map(canonical) ?? true, revision > 0 || after == nil
    else { throw TeraComposerAcknowledgment.unconfirmed }
    return .init(author: author, revision: revision, after: after)
  }
}

/// One pass owns this handle. Failed persistence never advances its local view.
actor TeraNativeRecoveryContinuation {
  private var value: TeraNativeRecoverySchedule
  private let client: TeraRuntimeClient

  init(_ value: TeraNativeRecoverySchedule, client: TeraRuntimeClient) {
    self.value = value
    self.client = client
  }

  func advance(_ after: String?) async throws {
    value = try await client.advanceNativeRecoverySchedule(expected: value, after: after)
  }
}
