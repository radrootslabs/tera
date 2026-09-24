@testable import TeraApp
import XCTest

extension TeraSupportingStoreTests {
    @MainActor
    func testCacheSettingsRejectIntegerOverflowWithoutReplacingAcceptedState() async throws {
        let backend = SupportingBackend()
        let client = try await Self.startedClient(backend)
        let store = TeraSettingsStore(runtimeClient: client)
        await store.load(profile: nil)
        for (megabytes, artifacts) in [(Int.max, 1), (Int.min, 1), (256, Int.max), (256, Int.min)] {
            store.mediaCacheMegabytes = megabytes
            store.mediaCacheArtifacts = artifacts
            let restart = await store.saveSettings()
            XCTAssertFalse(restart)
            XCTAssertNotNil(store.failureCode)
            let revision = await backend.settingsRevision()
            XCTAssertEqual(revision, 1)
        }
        _ = try await client.stop()
    }
}
