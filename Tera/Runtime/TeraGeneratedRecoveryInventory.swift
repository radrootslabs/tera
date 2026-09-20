import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  func recoveryPage(limit: UInt16, cursor: String?) async throws -> TeraRecoveryPage {
    do {
      let value = try await runtime.recoveryPage(schemaVersion: 1, limit: limit, cursor: cursor)
      guard value.schemaVersion == 1, limit > 0, limit <= 64, value.scanned <= limit,
            value.entries.count <= Int(value.scanned), Self.recoveryHex(value.author, count: 64),
            value.nextCursor?.isEmpty != true else { throw TeraComposerAcknowledgment.unconfirmed }
      let entries = try value.entries.map(Self.recoveryEntry)
      guard Set(entries.map(\.key)).count == entries.count,
            entries.map(\.key) == entries.map(\.key).sorted() else { throw TeraComposerAcknowledgment.unconfirmed }
      return TeraRecoveryPage(author: value.author, entries: entries, scanned: value.scanned, nextCursor: value.nextCursor)
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  func recoveryParent(key: String) async throws -> TeraRecoveryEntry? {
    do {
      guard let value = try await runtime.recoveryParent(schemaVersion: 1, key: key) else { return nil }
      let entry = try Self.recoveryEntry(value)
      guard entry.key == key else { throw TeraComposerAcknowledgment.unconfirmed }
      return entry
    } catch { throw TeraGeneratedRuntimeFailure.from(error) }
  }

  private static func recoveryEntry(_ value: FfiRecoveryEntry) throws -> TeraRecoveryEntry {
    guard value.schemaVersion == 1, recoveryHex(value.key, count: 32),
          value.revision > 0, value.revision <= UInt64(Int64.max) else { throw TeraComposerAcknowledgment.unconfirmed }
    let owner: TeraRecoveryOwner = switch value.owner {
    case .legacy: .legacy
    case let .submission(request): try .submission(TeraGeneratedSubmission.request(request))
    case .repair: .repair
    }
    return TeraRecoveryEntry(key: value.key, revision: value.revision, owner: owner)
  }

  private static func recoveryHex(_ value: String, count: Int) -> Bool {
    value.utf8.count == count && value.utf8.allSatisfy { (48 ... 57).contains($0) || (97 ... 102).contains($0) }
  }
}
