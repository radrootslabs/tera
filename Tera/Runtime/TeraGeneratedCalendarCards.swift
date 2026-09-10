import TeraKitBindings

extension FfiTodayCardRecord {
  func appValue() throws -> TeraTodayCard {
    guard schemaVersion == 2, (cardType == .event) == (calendarTiming != nil) else {
      throw TeraCalendarTiming.unsupported
    }
    return try TeraTodayCard(
      id: cardId,
      type: cardType.appValue,
      sourceEventID: sourceEventId,
      sourceAddress: sourceAddress,
      authorPublicKey: authorPublicKey,
      contractID: contractId,
      title: title,
      content: content,
      authoredAtUnixSeconds: authoredAtUnixS,
      effectiveAtUnixSeconds: effectiveAtUnixS,
      calendarTiming: calendarTiming?.appValue(),
      location: location,
      priceAmount: priceAmount,
      priceCurrency: priceCurrency,
      priceUnit: priceUnit,
      quantity: quantity,
      foodSummary: foodSummary,
      foodPublishedAtUnixSeconds: foodPublishedAtUnixS,
      foodStatus: foodStatus,
      contextRank: contextRank,
      inclusionReason: inclusionReason,
      media: media.map(\.appValue),
      lifecycle: lifecycle.appValue,
      rankDigest: rankDigest,
      authorProfile: authorProfile?.appValue,
      thread: thread.map(\.appValue),
      localOperationID: localOperationId,
      localOperationState: localOperationState
    )
  }
}
