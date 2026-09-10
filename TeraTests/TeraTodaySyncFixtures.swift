@testable import TeraApp

enum TeraTodaySyncFixtures {
  static func receipt(
    state: TeraTodayRelaySyncState = .complete,
    termination: TeraTodaySyncTermination = .complete,
    targets: [TeraTodayTargetSyncReceipt] = [],
    update: TeraTodayProjectionUpdate = .incremental,
    discovery: TeraTodayDiscoveryReceipt? = nil
  ) -> TeraTodaySyncReceipt {
    TeraTodaySyncReceipt(
      relayState: state, termination: termination, targets: targets,
      pagesFetched: 2, eventsObserved: 3, eventsAdmitted: 2, eventsRejected: 1,
      projection: TeraTodayRefreshReceipt(
        update: update, sourceEvents: 0, visibleCards: 0, profiles: 0,
        threadEntries: 0, contentGeneration: 1, changed: false
      ),
      discovery: discovery ?? TeraTodayDiscoveryReceipt(continuation: nil, hadIncompleteResponses: false)
    )
  }
}
