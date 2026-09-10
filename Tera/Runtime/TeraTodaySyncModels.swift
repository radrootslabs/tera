import Foundation

enum TeraTodayRelaySyncState: Sendable, Equatable {
  case complete, partial, offline
}

enum TeraTodaySyncTermination: Sendable, Equatable {
  case complete, pageLimit, deadline, cancelled, sourceFailed
}

enum TeraTodayTargetSyncState: Sendable, Equatable {
  case complete, partial, unavailable, failedRetryable, failedTerminal, cancelled
}

struct TeraTodayTargetPageSummary: Sendable, Equatable {
  let pagesObserved: UInt16
  let incompletePages: UInt16
  let missingOutcomePages: UInt16
  let lastIncomplete: TeraTodayTargetSyncState?
}

struct TeraTodayTargetSyncReceipt: Sendable, Equatable {
  let targetFingerprint: String
  let finalState: TeraTodayTargetSyncState?
  let summary: TeraTodayTargetPageSummary?
}

struct TeraTodaySyncReceipt: Sendable, Equatable {
  let relayState: TeraTodayRelaySyncState
  let termination: TeraTodaySyncTermination
  let targets: [TeraTodayTargetSyncReceipt]
  let pagesFetched: UInt16
  let eventsObserved: UInt64
  let eventsAdmitted: UInt64
  let eventsRejected: UInt64
  let projection: TeraTodayRefreshReceipt
}

struct TeraTodayRefreshReceipt: Sendable, Equatable {
  let update: TeraTodayProjectionUpdate
  let sourceEvents: UInt64
  let visibleCards: UInt64
  let profiles: UInt64
  let threadEntries: UInt64
  let contentGeneration: UInt64
  let changed: Bool
}
