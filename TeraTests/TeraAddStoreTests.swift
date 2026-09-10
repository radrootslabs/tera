import CryptoKit
import RadrootsKit
@testable import TeraApp
import XCTest

final class TeraAddStoreTests: XCTestCase {
  func testBackgroundUploadResumesReceiptRetriesAndCompletedStateWithoutDuplicateEnqueue()
    async throws
  {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)

    let first = try await coordinator.uploadInBackground(
      job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)),
      media: fixture.media
    )
    XCTAssertEqual(first.expectedRevision, 2)
    var counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 1)

    let replayed = try await coordinator.uploadInBackground(
      job: fixture.job(revision: 3, operation: String(repeating: "b", count: 32)),
      media: fixture.media
    )
    XCTAssertEqual(replayed.identifier, first.identifier)
    XCTAssertEqual(replayed.expectedRevision, 3)
    counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 1)

    try await coordinator.settleBackgroundUpload(identifier: replayed.identifier, accepted: true)
    let completed = try await coordinator.uploadInBackground(
      job: fixture.job(revision: 4, operation: String(repeating: "c", count: 32)),
      media: fixture.media
    )
    XCTAssertEqual(completed.identifier, first.identifier)
    counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 1)
    try await coordinator.settleBackgroundUpload(identifier: completed.identifier, accepted: true)
    counts = await transfer.counts
    XCTAssertEqual(counts.acceptedSettlement, 1)

    try await transfer.setState(.interrupted)
    let retried = try await coordinator.uploadInBackground(
      job: fixture.job(revision: 5, operation: String(repeating: "d", count: 32)),
      media: fixture.media
    )
    XCTAssertEqual(retried.identifier, first.identifier)
    counts = await transfer.counts
    XCTAssertEqual(counts.retry, 1)
    XCTAssertEqual(counts.enqueue, 1)
  }

  func testBackgroundUploadRejectsMismatchedAndAmbiguousPersistedRequests() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    let request = try fixture.request(job: job)
    let mismatched = try fixture.request(
      job: job,
      remoteURL: "http://127.0.0.1:3000/not-the-authorized-object.png"
    )
    try await transfer.seed(request: mismatched, state: .running)

    do {
      _ = try await coordinator.uploadInBackground(job: job, media: fixture.media)
      XCTFail("expected persisted-request mismatch")
    } catch let failure as TeraRuntimeFailure {
      XCTAssertEqual(failure.code, "ios.add.background_upload_mismatch")
    }
    var counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 0)

    await transfer.removeAll()
    let future = try fixture.request(
      job: fixture.job(revision: 3, operation: String(repeating: "f", count: 32))
    )
    try await transfer.seed(request: future, state: .running)
    do {
      _ = try await coordinator.uploadInBackground(job: job, media: fixture.media)
      XCTFail("expected future transfer identity rejection")
    } catch let failure as TeraRuntimeFailure {
      XCTAssertEqual(failure.code, "ios.add.background_upload_mismatch")
    }

    await transfer.removeAll()
    try await transfer.seed(request: request, state: .running)
    let second = try fixture.request(
      job: fixture.job(revision: 1, operation: String(repeating: "e", count: 32))
    )
    try await transfer.seed(request: second, state: .running)
    do {
      _ = try await coordinator.uploadInBackground(job: job, media: fixture.media)
      XCTFail("expected ambiguous persisted state")
    } catch let failure as TeraRuntimeFailure {
      XCTAssertEqual(failure.code, "ios.add.background_upload_ambiguous")
    }
    counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 0)
  }

  func testBackgroundUploadCancellationLeavesDurableWorkForRelaunch() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness(enqueueState: .running)
    let coordinator = fixture.coordinator(transfer: transfer)
    let firstJob = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))

    let task = Task {
      try await coordinator.uploadInBackground(job: firstJob, media: fixture.media)
    }
    let reachedSnapshot = await Self.waitUntil { await transfer.snapshotCount > 0 }
    XCTAssertTrue(reachedSnapshot)
    task.cancel()
    do {
      _ = try await task.value
      XCTFail("expected cancellation")
    } catch is CancellationError {}

    var counts = await transfer.counts
    XCTAssertEqual(counts.cancel, 0)
    XCTAssertEqual(counts.enqueue, 1)
    try await transfer.setState(.awaitingVerification)
    let replayed = try await coordinator.uploadInBackground(
      job: fixture.job(revision: 3, operation: String(repeating: "b", count: 32)),
      media: fixture.media
    )
    XCTAssertEqual(replayed.identifier, firstJob.transferIdentifier)
    counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 1)
  }

  func testBackgroundUploadCancellationBeforeEnqueueHasNoTransferSideEffect() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness(pause: .discovery)
    let coordinator = fixture.coordinator(transfer: transfer)
    let task = Task {
      try await coordinator.uploadInBackground(
        job: fixture.job(revision: 2, operation: String(repeating: "a", count: 32)),
        media: fixture.media
      )
    }
    let reachedDiscovery = await Self.waitUntil { await transfer.isPaused }
    XCTAssertTrue(reachedDiscovery)
    task.cancel()
    do {
      _ = try await task.value
      XCTFail("expected cancellation")
    } catch is CancellationError {}

    let counts = await transfer.counts
    let state = await transfer.state
    XCTAssertEqual(counts.enqueue, 0)
    XCTAssertNil(state)
  }

  func testBackgroundUploadCancellationAtReceiptBoundaryReplaysOnRelaunch() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness(pause: .snapshot)
    let coordinator = fixture.coordinator(transfer: transfer)
    let firstJob = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    let task = Task {
      try await coordinator.uploadInBackground(job: firstJob, media: fixture.media)
    }
    let reachedReceipt = await Self.waitUntil { await transfer.isPaused }
    XCTAssertTrue(reachedReceipt)
    task.cancel()
    do {
      _ = try await task.value
      XCTFail("expected cancellation")
    } catch is CancellationError {}

    var counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 1)
    XCTAssertEqual(counts.cancel, 0)
    await transfer.releasePause()
    let replayed = try await coordinator.uploadInBackground(
      job: fixture.job(revision: 3, operation: String(repeating: "b", count: 32)),
      media: fixture.media
    )
    XCTAssertEqual(replayed.identifier, firstJob.transferIdentifier)
    counts = await transfer.counts
    XCTAssertEqual(counts.enqueue, 1)
  }

  func testVerifiedRustDraftReconcilesAwaitingReceiptAfterRelaunch() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let transfer = BackgroundTransferHarness()
    let coordinator = fixture.coordinator(transfer: transfer)
    let job = fixture.job(revision: 2, operation: String(repeating: "a", count: 32))
    try await transfer.seed(request: fixture.request(job: job), state: .awaitingVerification)

    try await coordinator.reconcileBackgroundUploads(
      drafts: [fixture.draft(revision: 3, stage: .verified)]
    )

    let counts = await transfer.counts
    let state = await transfer.state
    XCTAssertEqual(counts.acceptedSettlement, 1)
    XCTAssertEqual(state, .completed)
  }

  func testBackgroundReconciliationRejectsDuplicateDraftInventory() async throws {
    let fixture = try BackgroundUploadFixture()
    defer { fixture.remove() }
    let coordinator = fixture.coordinator(transfer: BackgroundTransferHarness())
    let draft = fixture.draft(revision: 3, stage: .verified)

    do {
      try await coordinator.reconcileBackgroundUploads(drafts: [draft, draft])
      XCTFail("expected duplicate draft rejection")
    } catch let failure as TeraRuntimeFailure {
      XCTAssertEqual(failure.code, "ios.add.background_draft_ambiguous")
    }
  }

  @MainActor
  func testAllFiveFormsCompleteThroughRuntimeAndSubmittedSnapshotsFreeze() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(
      runtimeClient: client,
      media: AddMediaHarness()
    )
    await store.configure(snapshot: backend.snapshot())
    await store.start()

    for type in TeraAddCommandType.allCases {
      store.newDraft(type: type)
      configure(store, type: type)
      if type == .createPhotoUpdate {
        await store.importPhotos()
        XCTAssertEqual(store.form.media.count, 1)
        XCTAssertNil(store.form.media.first?.remoteURL)
      }
      await store.submit()
      XCTAssertEqual(store.activeDraft?.commandType, type)
      XCTAssertEqual(store.activeDraft?.state, .complete)
      XCTAssertEqual(store.activeDraft?.form, store.form)
      if type == .createPhotoUpdate {
        XCTAssertEqual(
          store.form.media.first?.remoteURL,
          "http://127.0.0.1:3000/\(String(repeating: "0", count: 64)).png"
        )
        let didUpload = await backend.didUploadMedia()
        XCTAssertTrue(didUpload)
      }
    }

    let frozen = store.form
    store.updateForm(\.content, "mutated after submit")
    store.selectType(.createUpdate)
    XCTAssertEqual(store.form, frozen)
    XCTAssertFalse(store.isFormEditable)
    XCTAssertEqual(store.drafts.filter { $0.kind == .add }.count, 5)
    _ = try await client.stop()
  }

  @MainActor
  func testProductSurfaceSnapshotAndSchemaInventoryAreExact() async throws {
    XCTAssertEqual(
      TeraProductSurfaceContract.snapshot,
      "today=update,photoUpdate,ask,event,foodAvailability|add=createUpdate,createPhotoUpdate,createAsk,createEvent,createFoodAvailability|support=context_picker,search,me,settings"
    )

    let validBackend = AddBackend()
    let validClient = try await Self.startedClient(validBackend)
    let validStore = TeraAddStore(runtimeClient: validClient)
    await validStore.configure(snapshot: validBackend.snapshot())
    await validStore.start()
    XCTAssertEqual(validStore.state, .ready)
    XCTAssertEqual(validStore.schemas.map(\.commandType), TeraAddCommandType.allCases)
    XCTAssertTrue(validStore.isProductReady)

    let unicodeIdentifierStore = TeraAddStore(
      runtimeClient: validClient,
      initialType: .createFoodAvailability,
      identifier: { String(repeating: "٠", count: 16) }
    )
    XCTAssertNil(unicodeIdentifierStore.form.identifier)
    _ = try await validClient.stop()

    let exact = TeraAddSchemaFixtures.schemas()
    var missingField = exact
    let food = exact[4]
    missingField[4] = TeraAddSchema(
      schemaVersion: food.schemaVersion,
      commandType: food.commandType,
      label: food.label,
      fields: Array(food.fields.dropLast())
    )
    let invalidInventories = [
      Array(exact.reversed()),
      Array(exact.dropLast()),
      exact + [exact[0]],
      missingField,
    ]
    for inventory in invalidInventories {
      let invalidBackend = AddBackend(schemas: inventory)
      let invalidClient = try await Self.startedClient(invalidBackend)
      let invalidStore = TeraAddStore(runtimeClient: invalidClient)
      await invalidStore.configure(snapshot: invalidBackend.snapshot())
      await invalidStore.start()
      XCTAssertEqual(
        invalidStore.state,
        .failed(TeraUserMessages.text(.addOperationFailed))
      )
      XCTAssertFalse(invalidStore.isProductReady)
      XCTAssertFalse(invalidStore.canSave)
      XCTAssertFalse(invalidStore.canSubmit)
      _ = try await invalidClient.stop()
    }
  }

  @MainActor
  func testOfflineSubmitPersistsQueuedSnapshotForRetryAndReopen() async throws {
    let backend = AddBackend(advanceOffline: true)
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Saved while the farm is offline")

    await store.submit()

    let queued = try XCTUnwrap(store.activeDraft)
    XCTAssertEqual(queued.state, .queued)
    XCTAssertEqual(queued.form?.content, "Saved while the farm is offline")
    XCTAssertTrue(store.message?.contains("Saved for retry") == true)
    store.reopen(queued)
    XCTAssertEqual(store.form, queued.form)
    XCTAssertFalse(store.isFormEditable)
    _ = try await client.stop()
  }

  @MainActor
  func testRevisionUsesLosslessRustOwnedReplacementPlan() async throws {
    let backend = AddBackend(advanceOffline: true)
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.selectType(.createFoodAvailability)
    configure(store, type: .createFoodAvailability)
    await store.save()
    let source = try XCTUnwrap(store.activeDraft)

    await store.retractAndRevise(Self.card(localOperationID: source.id))

    XCTAssertNil(store.activeDraft)
    XCTAssertEqual(store.form, source.form)
    store.updateForm(\.content, "Corrected harvest details")
    await store.submit()
    XCTAssertTrue(store.activeDraft?.isRevision == true)
    XCTAssertEqual(store.activeDraft?.state, .complete)
    let revisionPlanCount = await backend.revisionPlanCount()
    XCTAssertEqual(revisionPlanCount, 1)
    XCTAssertNil(store.drafts.first(where: { $0.kind == .retraction }))
    _ = try await client.stop()
  }

  @MainActor
  func testRetractionUsesTypedTargetAndCompletesThroughTheDurableOutbox() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(
      runtimeClient: client,
      identifier: { String(repeating: "f", count: 32) },
      clock: .fixed(unixSeconds: 1_800_000_200)
    )
    await store.configure(snapshot: backend.snapshot())
    await store.start()

    await store.retract(Self.card())

    XCTAssertEqual(store.activeDraft?.id, String(repeating: "f", count: 32))
    XCTAssertEqual(store.activeDraft?.kind, .retraction)
    XCTAssertEqual(store.activeDraft?.state, .complete)
    XCTAssertFalse(store.isFormEditable)
    XCTAssertFalse(store.canSave)
    let recordedRetraction = await backend.lastRetraction()
    let retraction = try XCTUnwrap(recordedRetraction)
    XCTAssertEqual(retraction.commandType, .createFoodAvailability)
    XCTAssertEqual(retraction.targetKind, 30402)
    XCTAssertEqual(retraction.targetCardID, String(repeating: "c", count: 64))
    XCTAssertEqual(retraction.targetEventID, String(repeating: "e", count: 64))
    XCTAssertEqual(retraction.targetAddress, Self.card().sourceAddress)
    XCTAssertEqual(retraction.reason, "Removed by author.")
    _ = try await client.stop()
  }

  @MainActor
  func testStopFencesSaveQueueAndAdvanceAndPermitsCleanRestart() async throws {
    for phase in AddDelayPhase.allCases {
      let backend = AddBackend(delayedPhase: phase)
      let client = try await Self.startedClient(backend)
      let store = TeraAddStore(runtimeClient: client)
      await store.configure(snapshot: backend.snapshot())
      await store.start()
      store.updateForm(\.content, "Restart after \(phase)")

      let submit = Task { await store.submit() }
      try await Task.sleep(nanoseconds: 2_000_000)
      let visibleAtStop = store.activeDraft
      store.stop()
      await submit.value

      XCTAssertEqual(store.activeDraft, visibleAtStop)
      XCTAssertFalse(store.isWorking)

      await store.start()
      XCTAssertEqual(store.state, .ready)
      if let durable = store.drafts.first {
        store.reopen(durable)
      }
      await store.submit()
      XCTAssertEqual(store.activeDraft?.state, .complete)
      XCTAssertFalse(store.isWorking)
      _ = try await client.stop()
    }
  }

  @MainActor
  func testStopDuringBackgroundUploadPermitsDurableRestart() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(
      runtimeClient: client,
      media: AddMediaHarness(delayFirstUpload: true)
    )
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)
    store.updateForm(\.content, "Restart the background upload")
    await store.importPhotos()

    let submit = Task { await store.submit() }
    try await Task.sleep(nanoseconds: 2_000_000)
    let visibleAtStop = store.activeDraft
    store.stop()
    await submit.value

    XCTAssertEqual(store.activeDraft, visibleAtStop)
    XCTAssertFalse(store.isWorking)

    await store.start()
    let durable = try XCTUnwrap(store.drafts.first)
    XCTAssertEqual(durable.state, .mediaUploading)
    store.reopen(durable)
    await store.submit()
    XCTAssertEqual(store.activeDraft?.state, .complete)
    XCTAssertEqual(store.activeDraft?.media.first?.stage, .verified)
    _ = try await client.stop()
  }

  @MainActor
  func testCancellationAfterDurableRustVerificationDoesNotRejectReceipt() async throws {
    let backend = AddBackend(delayAfterBackgroundCompletion: true)
    let client = try await Self.startedClient(backend)
    let media = AddMediaHarness()
    let store = TeraAddStore(runtimeClient: client, media: media)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)
    store.updateForm(\.content, "Unknown Rust completion outcome")
    await store.importPhotos()

    let submit = Task { await store.submit() }
    let persistedCompletion = await Self.waitUntil {
      await backend.didPersistBackgroundCompletion()
    }
    XCTAssertTrue(persistedCompletion)
    store.stop()
    await submit.value

    var settlements = await media.settlementValues()
    XCTAssertEqual(settlements, [])
    await store.start()
    XCTAssertEqual(store.drafts.first?.state, .readyToSign)
    XCTAssertEqual(store.drafts.first?.media.first?.stage, .verified)
    settlements = await media.settlementValues()
    let reconciliations = await media.reconciliationCount()
    XCTAssertEqual(settlements, [])
    XCTAssertEqual(reconciliations, 2)
    _ = try await client.stop()
  }

  @MainActor
  func testCancellationDuringSettlementReconcilesVerifiedDraftOnRelaunch() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let media = AddMediaHarness(delaySettlement: true)
    let store = TeraAddStore(runtimeClient: client, media: media)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)
    store.updateForm(\.content, "Unknown settlement outcome")
    await store.importPhotos()

    let submit = Task { await store.submit() }
    let reachedSettlement = await Self.waitUntil { await media.didBeginSettlement() }
    XCTAssertTrue(reachedSettlement)
    store.stop()
    await submit.value

    let settlements = await media.settlementValues()
    XCTAssertEqual(settlements, [])
    await store.start()
    XCTAssertEqual(store.drafts.first?.media.first?.stage, .verified)
    let reconciliations = await media.reconciliationCount()
    XCTAssertEqual(reconciliations, 2)
    _ = try await client.stop()
  }

  @MainActor
  func testPhotoServiceProbeSurfacesCanonicalEvidence() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client, media: AddMediaHarness())
    await store.configure(snapshot: backend.snapshot())

    await store.checkPhotoService()

    XCTAssertEqual(store.blossomEvidence?.state, "reachable")
    XCTAssertEqual(store.blossomEvidence?.configFingerprint, String(repeating: "f", count: 64))
    XCTAssertEqual(store.mediaSupport, .init(library: true, camera: true))
    XCTAssertEqual(store.message, "Photo service is reachable.")
    _ = try await client.stop()
  }

  @MainActor
  func testPhotoLimitRejectsTwentyFirstImportAndCapture() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client, media: AddMediaHarness())
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)

    for _ in 0 ..< 20 {
      await store.importPhotos()
    }
    XCTAssertEqual(store.form.media.count, 20)
    XCTAssertFalse(store.canAddMedia)

    await store.importPhotos()
    await store.capturePhoto()
    XCTAssertEqual(store.form.media.count, 20)
    _ = try await client.stop()
  }

  @MainActor
  func testSuspendAndResumePreserveLateDurableDraftCompletion() async throws {
    let backend = AddBackend(delayedPhase: .save)
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Background draft")

    let save = Task { await store.save() }
    try await Task.sleep(nanoseconds: 2_000_000)
    store.suspend()
    await store.start()
    await save.value

    XCTAssertEqual(store.activeDraft?.form?.content, "Background draft")
    XCTAssertEqual(store.message, "Draft saved on this device.")
    XCTAssertFalse(store.isWorking)
    _ = try await client.stop()
  }

  @MainActor
  func testSubmitWithoutWritableRelayVerifiesMediaAndPreservesDraftForRetry() async throws {
    let backend = AddBackend(includeWritableRelay: false)
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(
      runtimeClient: client,
      media: AddMediaHarness()
    )
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.selectType(.createPhotoUpdate)
    store.updateForm(\.content, "Carrots from today")
    await store.importPhotos()

    await store.submit()

    XCTAssertEqual(store.activeDraft?.state, .readyToSign)
    XCTAssertEqual(store.activeDraft?.media.first?.stage, .verified)
    XCTAssertEqual(
      store.message,
      "Photo verified and draft saved. Configure a writable relay to publish."
    )
    XCTAssertNil(store.lastFailureCode)
    _ = try await client.stop()
  }

  @MainActor
  func testSubmitRetainsRedactedFailureCodeForSupportAndAccessibility() async throws {
    let failure = TeraRuntimeFailure(
      schemaVersion: 1,
      code: "media_handle_unavailable",
      category: "validation",
      retryable: false,
      recoveryActions: [],
      operationID: "add.save",
      capabilityID: nil,
      safeMessage: "The request is invalid."
    )
    let backend = AddBackend(saveFailure: failure)
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Prepared media failure")

    await store.submit()

    XCTAssertEqual(store.message, TeraUserMessages.text(.addOperationFailed))
    XCTAssertEqual(store.lastFailureCode, failure.code)
    XCTAssertNil(store.activeDraft)
    _ = try await client.stop()
  }

  @MainActor
  private func configure(_ store: TeraAddStore, type: TeraAddCommandType) {
    switch type {
    case .createUpdate:
      store.updateForm(\.content, "Harvest update")
    case .createPhotoUpdate:
      store.updateForm(\.content, "Carrots from today")
    case .createAsk:
      store.updateForm(\.content, "Who has seed potatoes?")
    case .createEvent:
      XCTAssertEqual(store.form.identifier?.count, 32)
      XCTAssertNotNil(store.form.eventStartUnixSeconds)
      XCTAssertNotNil(store.form.eventEndUnixSeconds)
      XCTAssertNotNil(store.form.eventStartDate)
      XCTAssertNotNil(store.form.eventEndDate)
      store.updateForm(\.title, Optional("Market day"))
      store.updateForm(\.eventTiming, Optional(TeraEventTiming.timed))
      store.updateForm(\.location, Optional("Town square"))
    case .createFoodAvailability:
      XCTAssertEqual(store.form.identifier?.count, 32)
      store.updateForm(\.title, Optional("Carrots"))
      store.updateForm(\.summary, Optional("Fresh carrots"))
      store.updateForm(\.content, "Freshly picked")
      store.updateForm(\.location, Optional("Town square"))
      store.updateForm(\.priceAmount, Optional("3"))
      store.updateForm(\.currency, Optional("CAD"))
      store.updateForm(\.unit, Optional("lb"))
    }
  }

  private static func waitUntil(
    _ predicate: @escaping @Sendable () async -> Bool
  ) async -> Bool {
    for _ in 0 ..< 1000 {
      if await predicate() {
        return true
      }
      try? await Task.sleep(nanoseconds: 1_000_000)
    }
    return false
  }

  private static func startedClient(_ backend: AddBackend) async throws -> TeraRuntimeClient {
    let client = TeraRuntimeClient { _ in
      await TeraRuntimeBackendStart(backend: backend, snapshot: backend.snapshot())
    }
    _ = try await client.start(configuration: configuration())
    return client
  }

  private static func configuration() -> TeraRuntimeLaunchConfiguration {
    TeraRuntimeLaunchConfiguration(
      applicationSupportDirectory: "/tmp/radroots-add-tests",
      publicKeyHex: String(repeating: "ab", count: 32),
      sourceGenerationHex: String(repeating: "cd", count: 32),
      sourceGenerationCreatedAtUnixMilliseconds: 1,
      protectedData: .available,
      networkProfile: .simulator,
      writableRelays: ["ws://127.0.0.1:7447"],
      blossom: TeraBlossomEndpointConfiguration(
        hostKind: .simulator,
        endpointAuthority: .loopbackDevelopment,
        primaryOrigin: "http://127.0.0.1:3000",
        fallbackOrigins: []
      ),
      app: TeraRuntimeAppMetadata(
        bundleIdentifier: "org.radroots.add-tests",
        version: "0.1.0-alpha",
        buildNumber: "1",
        buildSHA: nil
      ),
      signerGeneration: "add-tests",
      signer: AddSigner(),
      adoptBootstrapSettings: false
    )
  }

  private static func card(localOperationID: String? = nil) -> TeraTodayCard {
    TeraTodayCard(
      id: String(repeating: "c", count: 64),
      type: .foodAvailability,
      sourceEventID: String(repeating: "e", count: 64),
      sourceAddress: "30402:\(String(repeating: "ab", count: 32)):carrots",
      authorPublicKey: String(repeating: "ab", count: 32),
      contractID: "radroots.food_availability.v1",
      title: "Carrots",
      content: "Freshly picked",
      authoredAtUnixSeconds: 1_800_000_000,
      effectiveAtUnixSeconds: 1_800_000_000,
      calendarTiming: nil,
      location: "Town square",
      priceAmount: "3",
      priceCurrency: "CAD",
      priceUnit: "lb",
      quantity: "12",
      foodSummary: "Fresh carrots",
      foodPublishedAtUnixSeconds: 1_799_999_900,
      foodStatus: "active",
      contextRank: 1,
      inclusionReason: "local",
      media: [],
      lifecycle: .active,
      rankDigest: nil,
      authorProfile: nil,
      thread: [],
      localOperationID: localOperationID,
      localOperationState: nil
    )
  }
}

private struct AddSigner: TeraRuntimeSigner {
  func availability() async -> TeraRuntimeSignerAvailability {
    .ready
  }

  func sign(_: TeraRuntimeSigningRequest) async -> TeraRuntimeSigningOutcome {
    .failed
  }
}

private actor AddMediaHarness: TeraAddMediaHandling {
  private let delayFirstUpload: Bool
  private let delaySettlement: Bool
  private var uploadAttempts = 0
  private var settlementStarted = false
  private var settlements: [Bool] = []
  private var reconciliations = 0
  private let item = TeraPreparedMedia(
    opaqueReference: "media:\(String(repeating: "0", count: 64))",
    remoteURL: nil,
    sha256: String(repeating: "0", count: 64),
    mediaType: "image/png",
    byteSize: 4,
    width: 2,
    height: 2,
    alt: "Carrots",
    preparedAtUnixSeconds: 1_800_000_000
  )

  init(delayFirstUpload: Bool = false, delaySettlement: Bool = false) {
    self.delayFirstUpload = delayFirstUpload
    self.delaySettlement = delaySettlement
  }

  func support() -> TeraAddMediaSupport {
    .init(library: true, camera: true)
  }

  func importImages(limit _: Int) -> [TeraPreparedMedia] {
    [item]
  }

  func captureImage() -> TeraPreparedMedia {
    item
  }

  func open(_ media: [TeraPreparedMedia]) throws -> TeraOpenedMedia {
    try TeraMediaFileFixture.open(media, bytes: Data(repeating: 0, count: 4))
  }

  func uploadInBackground(
    job: TeraNativeUploadJob,
    media _: TeraPreparedMedia
  ) async throws -> TeraAddBackgroundUploadReceipt {
    uploadAttempts += 1
    if delayFirstUpload, uploadAttempts == 1 {
      try await Task.sleep(nanoseconds: 50_000_000)
    }
    return TeraAddBackgroundUploadReceipt(
      identifier: "radroots.add.\(job.draft.id).\(job.draft.revision).\(job.operationID)",
      draftID: job.draft.id,
      expectedRevision: job.draft.revision,
      statusCode: 200,
      mediaType: "application/json",
      contentEncoding: nil,
      body: Data("{}".utf8)
    )
  }

  func settleBackgroundUpload(identifier _: String, accepted: Bool) async throws {
    settlementStarted = true
    if delaySettlement {
      try await Task.sleep(nanoseconds: 50_000_000)
    }
    settlements.append(accepted)
  }

  func reconcileBackgroundUploads(drafts _: [TeraDraftStatus]) {
    reconciliations += 1
  }

  func didBeginSettlement() -> Bool {
    settlementStarted
  }

  func settlementValues() -> [Bool] {
    settlements
  }

  func reconciliationCount() -> Int {
    reconciliations
  }
}

private enum AddDelayPhase: String, CaseIterable {
  case save
  case queue
  case advance
}

private actor AddBackend: TeraRuntimeBackend {
  private let savePause: ResourceTestPause?
  private let advanceOffline: Bool
  private let saveFailure: TeraRuntimeFailure?
  private let includeWritableRelay: Bool
  private let delayedPhase: AddDelayPhase?
  private let delayAfterBackgroundCompletion: Bool
  private let schemaInventory: [TeraAddSchema]
  private var values: [String: TeraDraftStatus] = [:]
  private var uploadedMedia = false
  private var revisionPlans = 0
  private var delayConsumed = false
  private var backgroundCompletionPersisted = false
  private var recordedRetraction: TeraRetractionDraftInput?
  private var closed = false

  init(
    savePause: ResourceTestPause? = nil,
    advanceOffline: Bool = false,
    saveFailure: TeraRuntimeFailure? = nil,
    includeWritableRelay: Bool = true,
    delayedPhase: AddDelayPhase? = nil,
    delayAfterBackgroundCompletion: Bool = false,
    schemas: [TeraAddSchema] = TeraAddSchemaFixtures.schemas()
  ) {
    self.advanceOffline = advanceOffline
    self.savePause = savePause
    self.saveFailure = saveFailure
    self.includeWritableRelay = includeWritableRelay
    self.delayedPhase = delayedPhase
    self.delayAfterBackgroundCompletion = delayAfterBackgroundCompletion
    schemaInventory = schemas
  }

  func snapshot() -> TeraRuntimeSnapshot {
    TeraRuntimeSnapshot(
      identity: TeraRuntimeIdentity(
        publicKeyHex: String(repeating: "ab", count: 32),
        hostSignerConfigured: true
      ),
      relay: TeraRelayStatus(
        profile: "simulator",
        state: "configured",
        readAvailability: "unobserved",
        writeAvailability: "unobserved",
        relays: includeWritableRelay
          ? [
            TeraRelayEndpointStatus(
              url: "ws://127.0.0.1:7447",
              access: .readWrite,
              readState: "unobserved",
              writeState: "unobserved",
              readLastAttemptUnixMilliseconds: nil,
              writeLastAttemptUnixMilliseconds: nil,
              readNextAttemptUnixMilliseconds: nil,
              writeNextAttemptUnixMilliseconds: nil
            ),
          ] : []
      ),
      blossomConfiguration: TeraBlossomConfigurationStatus(
        schemaVersion: 1,
        hostKind: "simulator",
        endpointAuthority: "loopback_development",
        primaryOrigin: "http://127.0.0.1:3000",
        fallbackOrigins: [],
        configFingerprint: String(repeating: "f", count: 64)
      ),
      blossomEvidence: nil,
      crateName: "tera_ffi",
      crateVersion: "0.1.0-alpha",
      isClosed: closed
    )
  }

  func todayPage(request _: TeraTodayPageRequest) throws -> TeraTodayPage {
    throw unsupported()
  }

  func refreshToday(
    context _: TeraLocalNetwork,
    nowUnixSeconds _: UInt64,
    update _: TeraTodayProjectionUpdate, backfillCursor _: String?
  ) throws -> TeraTodaySyncReceipt {
    throw unsupported()
  }

  func addSchemas() -> [TeraAddSchema] {
    schemaInventory
  }

  func saveAddIntent(
    input: TeraAddRuntimeInput,
    existingDraftID: String?,
    expectedRevision: UInt64?
  ) async throws -> TeraDraftStatus {
    await savePause?.wait()
    if let saveFailure {
      throw saveFailure
    }
    try await delayOnce(at: .save)
    let id = existingDraftID ?? String(format: "%032x", values.count + 1)
    let savedMedia = input.form.media.map {
      TeraPreparedMedia(
        opaqueReference: $0.opaqueReference,
        remoteURL: "http://127.0.0.1:3000/\($0.sha256).png",
        sha256: $0.sha256,
        mediaType: $0.mediaType,
        byteSize: $0.byteSize,
        width: $0.width,
        height: $0.height,
        alt: $0.alt,
        preparedAtUnixSeconds: $0.preparedAtUnixSeconds
      )
    }
    var storedForm = input.form
    storedForm.media = savedMedia
    let status = makeStatus(
      id: id,
      revision: (expectedRevision ?? 0) + 1,
      kind: .add,
      commandType: input.form.commandType,
      form: storedForm,
      state: savedMedia.isEmpty ? .draft : .mediaPreparing,
      updatedAt: 1_800_000_000_000 + UInt64(values.count),
      media: savedMedia.map {
        TeraDraftMediaStatus(
          url: $0.remoteURL!,
          stage: .pending,
          uploadAttempts: 0,
          verifiedAtUnixMilliseconds: nil,
          possibleOrphan: false,
          orphanReasonCode: nil,
          orphanRecordedAtUnixMilliseconds: nil
        )
      }
    )
    values[id] = status
    return status
  }

  func probeBlossom() -> TeraBlossomEvidence {
    TeraBlossomEvidence(
      schemaVersion: 2,
      origin: "http://127.0.0.1:3000",
      configFingerprint: String(repeating: "f", count: 64),
      state: "reachable",
      lastSuccessfulState: "probe",
      transportSecurity: "loopback_plaintext",
      observedAtUnixMilliseconds: 1_800_000_000_000,
      httpStatus: 404,
      errorCode: nil,
      serverErrorCode: nil,
      errorPhase: nil,
      retryable: false,
      possibleOrphan: false,
      attempts: 1
    )
  }

  func saveRetractionDraft(
    id: String,
    input: TeraRetractionDraftInput,
    authoredAtUnixSeconds _: UInt64,
    persistedAtUnixMilliseconds: UInt64
  ) -> TeraDraftStatus {
    recordedRetraction = input
    let status = makeStatus(
      id: id,
      revision: 1,
      kind: .retraction,
      commandType: input.commandType,
      form: nil,
      state: .draft,
      updatedAt: persistedAtUnixMilliseconds,
      media: []
    )
    values[id] = status
    return status
  }

  func lastRetraction() -> TeraRetractionDraftInput? {
    recordedRetraction
  }

  func saveRevisionIntent(
    target: TeraRevisionTarget,
    replacement: TeraAddRuntimeInput
  ) -> TeraRevisionStatus {
    revisionPlans += 1
    let id = String(format: "%032x", values.count + 1)
    let status = makeStatus(
      id: id,
      revision: 1,
      kind: .add,
      commandType: replacement.form.commandType,
      form: replacement.form,
      state: replacement.form.media.isEmpty ? .draft : .mediaPreparing,
      updatedAt: 1_800_000_100_000,
      media: [],
      isRevision: true
    )
    values[id] = status
    return TeraRevisionStatus(
      operationID: id,
      replacement: status,
      retraction: nil,
      policy: target.sourceAddress == nil ? .replaceThenRetract : .addressableReplacement,
      phase: .replacementPending
    )
  }

  func revisionStatus(operationID: String) throws -> TeraRevisionStatus {
    let replacement = try draftStatus(id: operationID)
    return TeraRevisionStatus(
      operationID: operationID,
      replacement: replacement,
      retraction: nil,
      policy: .addressableReplacement,
      phase: replacement.state == .complete ? .complete : .replacementPending
    )
  }

  func advanceRevision(operationID: String) throws -> TeraRevisionStatus {
    let current = try draftStatus(id: operationID)
    let completed = replacing(
      current,
      revision: current.revision + 1,
      state: .complete,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[operationID] = completed
    return try revisionStatus(operationID: operationID)
  }

  func cancelRevision(operationID: String) throws -> TeraRevisionStatus {
    let current = try draftStatus(id: operationID)
    let cancelled = replacing(
      current,
      revision: current.revision + 1,
      state: .cancelled,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[operationID] = cancelled
    return TeraRevisionStatus(
      operationID: operationID,
      replacement: cancelled,
      retraction: nil,
      policy: .addressableReplacement,
      phase: .cancelled
    )
  }

  func draftStatus(id: String) throws -> TeraDraftStatus {
    try storedDraft(id: id)
  }

  func draftHeads(limit: UInt16) -> [TeraDraftStatus] {
    Array(values.values.prefix(Int(limit)))
  }

  func queueAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> TeraDraftStatus {
    try await delayOnce(at: .queue)
    guard includeWritableRelay else {
      throw TeraRuntimeFailure(
        schemaVersion: 1,
        code: "writable_relay_unavailable",
        category: "relay",
        retryable: true,
        recoveryActions: ["configure_relay", "retry"],
        operationID: id,
        capabilityID: "nostr_sink",
        safeMessage: "No writable relay is configured."
      )
    }
    let current = try storedDraft(id: id)
    guard current.revision == expectedRevision else { throw unsupported() }
    let value = replacing(
      current, revision: current.revision + 1, state: .queued,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[id] = value
    return value
  }

  func recoverAddIntent(id: String) throws -> TeraDraftStatus {
    let current = try draftStatus(id: id)
    let value = replacing(
      current, revision: current.revision + 1, state: .queued,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[id] = value
    return value
  }

  func uploadAddMediaIntent(input: TeraBlossomUploadIntent) throws -> TeraDraftStatus {
    uploadedMedia = true
    let current = try storedDraft(id: input.draftID)
    let verified = current.media.map {
      TeraDraftMediaStatus(
        url: $0.url,
        stage: .verified,
        uploadAttempts: $0.uploadAttempts + 1,
        verifiedAtUnixMilliseconds: current.updatedAtUnixMilliseconds + 1,
        possibleOrphan: false,
        orphanReasonCode: nil,
        orphanRecordedAtUnixMilliseconds: nil
      )
    }
    let value = replacing(
      current,
      revision: current.revision + 1,
      state: .readyToSign,
      updatedAt: current.updatedAtUnixMilliseconds + 1,
      media: verified
    )
    values[current.id] = value
    return value
  }

  func prepareAddMediaBackground(
    input: TeraBlossomUploadIntent
  ) throws -> TeraNativeUploadJob {
    let current = try draftStatus(id: input.draftID)
    let uploading = replacing(
      current,
      revision: current.revision + 1,
      state: .mediaUploading,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[current.id] = uploading
    return TeraNativeUploadJob(
      operationID: String(repeating: "a", count: 32),
      draft: uploading,
      remoteURL: "http://127.0.0.1:3000/\(input.media.media.sha256).png",
      authorizationHeader: "Nostr test",
      expectedSHA256: input.media.media.sha256,
      mediaType: input.media.media.mediaType,
      byteSize: input.media.media.byteSize
    )
  }

  func completeAddMediaBackground(
    input: TeraNativeUploadCompletion
  ) async throws -> TeraDraftStatus {
    uploadedMedia = true
    let current = try storedDraft(id: input.draftID)
    let verified = current.media.map {
      TeraDraftMediaStatus(
        url: $0.url,
        stage: .verified,
        uploadAttempts: $0.uploadAttempts + 1,
        verifiedAtUnixMilliseconds: current.updatedAtUnixMilliseconds + 1,
        possibleOrphan: false,
        orphanReasonCode: nil,
        orphanRecordedAtUnixMilliseconds: nil
      )
    }
    let value = replacing(
      current,
      revision: current.revision + 1,
      state: .readyToSign,
      updatedAt: current.updatedAtUnixMilliseconds + 1,
      media: verified
    )
    values[current.id] = value
    backgroundCompletionPersisted = true
    if delayAfterBackgroundCompletion {
      try await Task.sleep(nanoseconds: 50_000_000)
    }
    return value
  }

  func didPersistBackgroundCompletion() -> Bool {
    backgroundCompletionPersisted
  }

  func didUploadMedia() -> Bool {
    uploadedMedia
  }

  func revisionPlanCount() -> Int {
    revisionPlans
  }

  func advanceDraft(id: String, expectedRevision: UInt64) async throws -> TeraDraftStatus {
    try await delayOnce(at: .advance)
    if advanceOffline {
      throw TeraRuntimeFailure(
        schemaVersion: 1,
        code: "test.offline",
        category: "relay",
        retryable: true,
        recoveryActions: ["retry"],
        operationID: id,
        capabilityID: "nostr_sink",
        safeMessage: "The relay is offline."
      )
    }
    let current = try storedDraft(id: id)
    let value = replacing(
      current, revision: expectedRevision, state: .complete,
      updatedAt: current.updatedAtUnixMilliseconds
    )
    values[id] = value
    return value
  }

  func cancelAddIntent(
    id: String,
    expectedRevision _: UInt64
  ) throws -> TeraDraftStatus {
    let current = try draftStatus(id: id)
    let value = replacing(
      current, revision: current.revision + 1, state: .cancelled,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[id] = value
    return value
  }

  func subscribe(
    bufferCapacity _: Int,
    receive _: @escaping @Sendable (TeraRuntimeChange) async -> Void
  ) -> any TeraRuntimeSubscriptionToken {
    AddSubscriptionToken()
  }

  func shutdown() -> TeraRuntimeShutdownReceipt {
    let wasClosed = closed
    closed = true
    return TeraRuntimeShutdownReceipt(state: "closed", alreadyClosed: wasClosed)
  }

  private func makeStatus(
    id: String,
    revision: UInt64,
    kind: TeraDraftKind,
    commandType: TeraAddCommandType,
    form: TeraAddForm?,
    state: TeraOutboxState,
    updatedAt: UInt64,
    media: [TeraDraftMediaStatus],
    isRevision: Bool = false
  ) -> TeraDraftStatus {
    TeraDraftStatus(
      id: id,
      revision: revision,
      authorPublicKey: String(repeating: "ab", count: 32),
      kind: kind,
      commandType: commandType,
      form: form,
      state: state,
      cardID: String(repeating: "c", count: 64),
      operationID: state == .draft ? nil : String(repeating: "d", count: 32),
      createdAtUnixMilliseconds: updatedAt,
      updatedAtUnixMilliseconds: updatedAt,
      media: media,
      settlement: state == .complete ? settlement() : nil,
      isRevision: isRevision
    )
  }

  private func replacing(
    _ value: TeraDraftStatus,
    revision: UInt64,
    state: TeraOutboxState,
    updatedAt: UInt64,
    media: [TeraDraftMediaStatus]? = nil
  ) -> TeraDraftStatus {
    TeraDraftStatus(
      id: value.id,
      revision: revision,
      authorPublicKey: value.authorPublicKey,
      kind: value.kind,
      commandType: value.commandType,
      form: value.form,
      state: state,
      cardID: value.cardID,
      operationID: state == .draft ? nil : String(repeating: "d", count: 32),
      createdAtUnixMilliseconds: value.createdAtUnixMilliseconds,
      updatedAtUnixMilliseconds: updatedAt,
      media: media ?? value.media,
      settlement: state == .complete ? settlement() : nil,
      isRevision: value.isRevision
    )
  }

  private func settlement() -> TeraOperationSettlement {
    TeraOperationSettlement(
      artifacts: 1,
      signed: 1,
      admitted: 1,
      pending: 0,
      retryable: 0,
      indeterminate: 0,
      failedTerminal: 0,
      cancelled: 0,
      deliveryPlans: 1,
      deliverySatisfied: 1,
      deliveryPending: 0,
      deliveryRetryable: 0,
      deliveryExhausted: 0,
      deliveryFailedTerminal: 0,
      deliveryCancelled: 0
    )
  }

  private func delayOnce(at phase: AddDelayPhase) async throws {
    guard delayedPhase == phase, !delayConsumed else { return }
    delayConsumed = true
    try await Task.sleep(nanoseconds: 50_000_000)
  }

  private func storedDraft(id: String) throws -> TeraDraftStatus {
    guard let value = values[id] else { throw unsupported() }
    return value
  }

  private func unsupported() -> TeraRuntimeFailure {
    .local(
      operation: "test.add", code: "test.unsupported", safeMessage: "Unsupported test operation."
    )
  }
}

private actor AddSubscriptionToken: TeraRuntimeSubscriptionToken {
  func cancel() {}
}

extension TeraAddStoreTests {
  @MainActor
  func testMutationAdmissionKeepsOneAddOperationBeforeItsBackendWait() async throws {
    let pause = ResourceTestPause()
    let backend = AddBackend(savePause: pause)
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "One admitted draft")
    let owner = Task { await store.save() }
    await admissionPauseEntered(pause)
    XCTAssertTrue(store.isWorking)
    await store.save()
    await store.submit()
    XCTAssertTrue(store.isWorking)
    XCTAssertNil(store.activeDraft)
    await pause.resume.open()
    await owner.value
    XCTAssertEqual(store.drafts.count, 1)
    XCTAssertEqual(store.activeDraft?.revision, 1)
    XCTAssertEqual(store.activeDraft?.state, .draft)
    XCTAssertFalse(store.isWorking)
    _ = try await client.stop()
  }

  @MainActor
  func testMutationAdmissionIgnoresAnAddCallerCancelledWhileQueued() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Only save after explicit admission")
    let pause = ResourceTestPause()
    let queued = Task { await pause.wait(); await store.save() }
    await admissionPauseEntered(pause)
    queued.cancel()
    await pause.resume.open()
    await queued.value
    XCTAssertTrue(store.drafts.isEmpty)
    XCTAssertFalse(store.isWorking)
    await store.save()
    XCTAssertEqual(store.drafts.count, 1)
    _ = try await client.stop()
  }

  @MainActor
  private func admissionPauseEntered(_ pause: ResourceTestPause) async {
    let entered = expectation(description: "Entered the explicit Add pause")
    let observer = Task { await pause.entered.wait(); entered.fulfill() }
    await fulfillment(of: [entered], timeout: 2)
    await pause.entered.open()
    await observer.value
  }
}
