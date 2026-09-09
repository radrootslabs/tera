/// A transient callback guard. This value is never a persisted operation key.
struct TeraSessionGeneration: Sendable, Equatable, Hashable {
  private let value: UInt64?

  static let initial = Self(rawValue: 0)

  init(rawValue: UInt64) {
    value = rawValue
  }

  private init(exhausted _: Void) {
    value = nil
  }

  var isActive: Bool {
    value != nil
  }

  var diagnosticValue: String {
    value.map(String.init) ?? "exhausted"
  }

  func requireActive() throws -> Self {
    guard isActive else {
      throw TeraStateTransitionError.generationOverflow
    }
    return self
  }

  func next() throws -> Self {
    guard let value else {
      throw TeraStateTransitionError.generationOverflow
    }
    return try Self(rawValue: TeraCheckedStateTransition.nextGeneration(after: value))
  }

  /// Cleanup must invalidate old callbacks even when no further session can start.
  func invalidated() -> Self {
    (try? next()) ?? Self(exhausted: ())
  }
}

/// Diagnostic identity of an in-memory call, never the authored operation ID.
struct TeraRuntimeOperationIdentity: Sendable, Equatable, Hashable {
  let generation: TeraSessionGeneration
  let sequence: UInt64
  let kind: TeraRuntimeOperationKind

  var rawValue: String {
    "ios-runtime-\(generation.diagnosticValue)-\(sequence)-\(kind.rawValue)"
  }
}

/// Revision carried by a Rust invalidation hint, distinct from host sessions.
struct TeraProjectionRevision: Sendable, Equatable, Hashable {
  let rawValue: UInt64?

  static let exhausted = Self(rawValue: nil)
}
