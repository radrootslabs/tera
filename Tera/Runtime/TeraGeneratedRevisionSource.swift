import TeraKitBindings

extension TeraGeneratedRuntimeBackend {
  func saveRevisionIntent(
    requestID: String,
    target: TeraRevisionTarget,
    replacement: TeraAddRuntimeInput
  ) async throws -> TeraRevisionStatus {
    do {
      return try await runtime.phase1SaveRevisionIntent(
        input: FfiRevisionInputRecord(
          requestId: requestID,
          schemaVersion: 1,
          cardId: target.cardID,
          sourceEventId: target.sourceEventID,
          sourceAddress: target.sourceAddress,
          authorPublicKey: target.authorPublicKey,
          replacement: replacement.generatedValue
        )
      ).appValue
    } catch {
      throw TeraGeneratedRuntimeFailure.from(error)
    }
  }

  func revisionStatus(operationID: String) async throws -> TeraRevisionStatus {
    do {
      return try await runtime.phase1RevisionStatus(operationId: operationID).appValue
    } catch {
      throw TeraGeneratedRuntimeFailure.from(error)
    }
  }

  func advanceRevision(operationID: String) async throws -> TeraRevisionStatus {
    do {
      return try await runtime.phase1AdvanceRevision(operationId: operationID).appValue
    } catch {
      throw TeraGeneratedRuntimeFailure.from(error)
    }
  }

  func cancelRevision(operationID: String) async throws -> TeraRevisionStatus {
    do {
      return try await runtime.phase1CancelRevision(operationId: operationID).appValue
    } catch {
      throw TeraGeneratedRuntimeFailure.from(error)
    }
  }

  func revisionSourceForm(card: TeraTodayCard, sourceDraftID: String) async throws -> TeraAddForm {
    do {
      return try await runtime.revisionSourceForm(request: FfiRevisionSourceRequest(
        schemaVersion: 1, sourceDraftId: sourceDraftID, commandType: card.type.addCommandType.generatedValue,
        cardId: card.id, sourceEventId: card.sourceEventID, sourceAddress: card.sourceAddress,
        authorPublicKey: card.authorPublicKey
      )).appValue
    } catch {
      throw TeraGeneratedRuntimeFailure.from(error)
    }
  }
}
