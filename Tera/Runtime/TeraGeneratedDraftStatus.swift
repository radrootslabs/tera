import TeraKitBindings

extension FfiDraftStatusRecord {
  var appValue: TeraDraftStatus {
    TeraDraftStatus(
      id: draftId,
      revision: revision,
      authorPublicKey: authorPublicKey,
      kind: kind.appValue,
      commandType: commandType.appValue,
      form: form?.appValue,
      state: state.appValue,
      cardID: cardId,
      operationID: operationId,
      createdAtUnixMilliseconds: createdAtUnixMs,
      updatedAtUnixMilliseconds: updatedAtUnixMs,
      media: media.map(\.appValue),
      settlement: settlement?.appValue,
      isRevision: isRevision,
      revisionParentID: revisionParentDraftId,
      coordinateWritable: coordinateWritable,
      coordinateCaptured: coordinateCaptured
    )
  }
}
