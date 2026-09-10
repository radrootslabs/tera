import Foundation
import TeraKitBindings

extension FfiTodaySyncRecord {
  var appValue: TeraTodaySyncReceipt {
    TeraTodaySyncReceipt(
      relayState: relayState.appValue, termination: termination.appValue,
      targets: targets.map(\.appValue), pagesFetched: pagesFetched,
      eventsObserved: eventsObserved, eventsAdmitted: eventsAdmitted,
      eventsRejected: eventsRejected, projection: projection.appValue
    )
  }
}

extension FfiTodayTargetSyncRecord {
  var appValue: TeraTodayTargetSyncReceipt {
    TeraTodayTargetSyncReceipt(
      targetFingerprint: targetFingerprint, finalState: finalState?.appValue, summary: summary?.appValue
    )
  }
}

extension FfiTodayTargetPageSummary {
  var appValue: TeraTodayTargetPageSummary {
    TeraTodayTargetPageSummary(
      pagesObserved: pagesObserved, incompletePages: incompletePages,
      missingOutcomePages: missingOutcomePages, lastIncomplete: lastIncomplete?.appValue
    )
  }
}

extension FfiTodayRelaySyncState {
  var appValue: TeraTodayRelaySyncState {
    switch self {
    case .complete: .complete
    case .partial: .partial
    case .offline: .offline
    }
  }
}

extension FfiTodaySyncTermination {
  var appValue: TeraTodaySyncTermination {
    switch self {
    case .complete: .complete
    case .pageLimit: .pageLimit
    case .deadline: .deadline
    case .cancelled: .cancelled
    case .sourceFailed: .sourceFailed
    }
  }
}

extension FfiTodayTargetSyncState {
  var appValue: TeraTodayTargetSyncState {
    switch self {
    case .complete: .complete
    case .partial: .partial
    case .unavailable: .unavailable
    case .failedRetryable: .failedRetryable
    case .failedTerminal: .failedTerminal
    case .cancelled: .cancelled
    }
  }
}

extension FfiTodayRefreshRecord {
  var appValue: TeraTodayRefreshReceipt {
    TeraTodayRefreshReceipt(
      update: update.appValue,
      sourceEvents: sourceEvents,
      visibleCards: visibleCards,
      profiles: profiles,
      threadEntries: threadEntries,
      contentGeneration: contentGeneration,
      changed: changed
    )
  }
}

extension FfiTodayProjectionUpdate {
  var appValue: TeraTodayProjectionUpdate {
    switch self {
    case .incremental: .incremental
    case .rebuild: .rebuild
    }
  }
}
