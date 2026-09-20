import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  func nativeRecoveryStatus(key: String) async throws -> TeraNativeRecoveryStatus? {
    do {
      return try await runtime.nativeRecoveryStatus(schemaVersion: 1, transferKey: key).map { try $0.appValue(key: key) }
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func reportNativeRecoveryStatus(key: String, reason: TeraNativeRecoveryReason) async throws -> TeraNativeRecoveryStatus? {
    do {
      let value = try await runtime.reportNativeRecoveryStatus(schemaVersion: 1, transferKey: key, reason: reason.generatedValue)
      let result = try value.map { try $0.appValue(key: key) }
      guard result?.reason == reason || (result == nil && reason == .resolved) else { throw TeraComposerAcknowledgment.unconfirmed }
      return result
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }
}

private extension FfiNativeRecoveryStatus {
  func appValue(key: String) throws -> TeraNativeRecoveryStatus {
    guard schemaVersion == 1, self.key == key, revision > 0, revision <= UInt64(Int64.max),
          firstObservedUnixMs > 0, updatedAtUnixMs >= firstObservedUnixMs, updatedAtUnixMs <= UInt64(Int64.max)
    else { throw TeraComposerAcknowledgment.unconfirmed }
    return .init(key: key, reason: reason.appValue, revision: revision,
                 firstObservedUnixMS: firstObservedUnixMs, updatedAtUnixMS: updatedAtUnixMs)
  }
}

private extension FfiNativeRecoveryReason {
  var appValue: TeraNativeRecoveryReason {
    switch self {
    case .missingParent: .missingParent
    case .invalidParent: .invalidParent
    case .associationMismatch: .associationMismatch
    case .outcomeUnconfirmed: .outcomeUnconfirmed
    case .resolved: .resolved
    }
  }
}

private extension TeraNativeRecoveryReason {
  var generatedValue: FfiNativeRecoveryReason {
    switch self {
    case .missingParent: .missingParent
    case .invalidParent: .invalidParent
    case .associationMismatch: .associationMismatch
    case .outcomeUnconfirmed: .outcomeUnconfirmed
    case .resolved: .resolved
    }
  }
}
