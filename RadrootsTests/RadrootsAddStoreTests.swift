import CryptoKit
@testable import RadrootsApp
import RadrootsKit
import XCTest

final class RadrootsAddStoreTests: XCTestCase {
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
    } catch let failure as RadrootsRuntimeFailure {
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
    } catch let failure as RadrootsRuntimeFailure {
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
    } catch let failure as RadrootsRuntimeFailure {
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
    } catch let failure as RadrootsRuntimeFailure {
      XCTAssertEqual(failure.code, "ios.add.background_draft_ambiguous")
    }
  }

  @MainActor
  func testAllFiveFormsCompleteThroughRuntimeAndSubmittedSnapshotsFreeze() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = RadrootsAddStore(
      runtimeClient: client,
      media: AddMediaHarness()
    )
    await store.configure(snapshot: backend.snapshot())
    await store.start()

    for type in RadrootsAddCommandType.allCases {
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
      RadrootsProductSurfaceContract.snapshot,
      "today=update,photoUpdate,ask,event,foodAvailability|add=createUpdate,createPhotoUpdate,createAsk,createEvent,createFoodAvailability|support=context_picker,search,me,settings"
    )

    let validBackend = AddBackend()
    let validClient = try await Self.startedClient(validBackend)
    let validStore = RadrootsAddStore(runtimeClient: validClient)
    await validStore.configure(snapshot: validBackend.snapshot())
    await validStore.start()
    XCTAssertEqual(validStore.state, .ready)
    XCTAssertEqual(validStore.schemas.map(\.commandType), RadrootsAddCommandType.allCases)
    XCTAssertTrue(validStore.isProductReady)

    let unicodeIdentifierStore = RadrootsAddStore(
      runtimeClient: validClient,
      initialType: .createFoodAvailability,
      identifier: { String(repeating: "٠", count: 16) }
    )
    XCTAssertNil(unicodeIdentifierStore.form.identifier)
    _ = try await validClient.stop()

    let exact = AddBackend.schemas()
    var missingField = exact
    let food = exact[4]
    missingField[4] = RadrootsAddSchema(
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
      let invalidStore = RadrootsAddStore(runtimeClient: invalidClient)
      await invalidStore.configure(snapshot: invalidBackend.snapshot())
      await invalidStore.start()
      XCTAssertEqual(
        invalidStore.state,
        .failed(RadrootsUserMessages.text(.addOperationFailed))
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
    let store = RadrootsAddStore(runtimeClient: client)
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
    let store = RadrootsAddStore(runtimeClient: client)
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
    let store = RadrootsAddStore(
      runtimeClient: client,
      identifier: { String(repeating: "f", count: 32) },
      nowUnixSeconds: { 1_800_000_200 },
      nowUnixMilliseconds: { 1_800_000_200_000 }
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
      let store = RadrootsAddStore(runtimeClient: client)
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
    let store = RadrootsAddStore(
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
    let store = RadrootsAddStore(runtimeClient: client, media: media)
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
    let store = RadrootsAddStore(runtimeClient: client, media: media)
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
    let store = RadrootsAddStore(runtimeClient: client, media: AddMediaHarness())
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
    let store = RadrootsAddStore(runtimeClient: client, media: AddMediaHarness())
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
    let store = RadrootsAddStore(runtimeClient: client)
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
    let store = RadrootsAddStore(
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
    let failure = RadrootsRuntimeFailure(
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
    let store = RadrootsAddStore(runtimeClient: client)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    store.updateForm(\.content, "Prepared media failure")

    await store.submit()

    XCTAssertEqual(store.message, RadrootsUserMessages.text(.addOperationFailed))
    XCTAssertEqual(store.lastFailureCode, failure.code)
    XCTAssertNil(store.activeDraft)
    _ = try await client.stop()
  }

  @MainActor
  private func configure(_ store: RadrootsAddStore, type: RadrootsAddCommandType) {
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
      store.updateForm(\.eventTiming, Optional(RadrootsEventTiming.timed))
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

  private static func startedClient(_ backend: AddBackend) async throws -> RadrootsRuntimeClient {
    let client = RadrootsRuntimeClient { _ in
      await RadrootsRuntimeBackendStart(backend: backend, snapshot: backend.snapshot())
    }
    _ = try await client.start(configuration: configuration())
    return client
  }

  private static func configuration() -> RadrootsRuntimeLaunchConfiguration {
    RadrootsRuntimeLaunchConfiguration(
      applicationSupportDirectory: "/tmp/radroots-add-tests",
      publicKeyHex: String(repeating: "ab", count: 32),
      sourceGenerationHex: String(repeating: "cd", count: 32),
      sourceGenerationCreatedAtUnixMilliseconds: 1,
      protectedData: .available,
      networkProfile: .simulator,
      writableRelays: ["ws://127.0.0.1:7447"],
      blossom: RadrootsBlossomEndpointConfiguration(
        hostKind: .simulator,
        endpointAuthority: .loopbackDevelopment,
        primaryOrigin: "http://127.0.0.1:3000",
        fallbackOrigins: []
      ),
      app: RadrootsRuntimeAppMetadata(
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

  private static func card(localOperationID: String? = nil) -> RadrootsTodayCard {
    RadrootsTodayCard(
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
      eventStartUnixSeconds: nil,
      eventEndUnixSeconds: nil,
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

private struct AddSigner: RadrootsRuntimeSigner {
  func availability() async -> RadrootsRuntimeSignerAvailability {
    .ready
  }

  func sign(_: RadrootsRuntimeSigningRequest) async -> RadrootsRuntimeSigningOutcome {
    .failed
  }
}

private actor AddMediaHarness: RadrootsAddMediaHandling {
  private let delayFirstUpload: Bool
  private let delaySettlement: Bool
  private var uploadAttempts = 0
  private var settlementStarted = false
  private var settlements: [Bool] = []
  private var reconciliations = 0
  private let item = RadrootsPreparedMedia(
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

  func support() -> RadrootsAddMediaSupport {
    .init(library: true, camera: true)
  }

  func importImages(limit _: Int) -> [RadrootsPreparedMedia] {
    [item]
  }

  func captureImage() -> RadrootsPreparedMedia {
    item
  }

  func open(_ media: [RadrootsPreparedMedia]) -> RadrootsOpenedMedia {
    RadrootsOpenedMedia(
      handles: media.map { RadrootsPreparedMediaHandle(media: $0, fileDescriptor: 1) },
      files: []
    )
  }

  func uploadInBackground(
    job: RadrootsNativeUploadJob,
    media _: RadrootsPreparedMedia
  ) async throws -> RadrootsAddBackgroundUploadReceipt {
    uploadAttempts += 1
    if delayFirstUpload, uploadAttempts == 1 {
      try await Task.sleep(nanoseconds: 50_000_000)
    }
    return RadrootsAddBackgroundUploadReceipt(
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

  func reconcileBackgroundUploads(drafts _: [RadrootsDraftStatus]) {
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

private actor AddBackend: RadrootsRuntimeBackend {
  private let advanceOffline: Bool
  private let saveFailure: RadrootsRuntimeFailure?
  private let includeWritableRelay: Bool
  private let delayedPhase: AddDelayPhase?
  private let delayAfterBackgroundCompletion: Bool
  private let schemaInventory: [RadrootsAddSchema]
  private var values: [String: RadrootsDraftStatus] = [:]
  private var uploadedMedia = false
  private var revisionPlans = 0
  private var delayConsumed = false
  private var backgroundCompletionPersisted = false
  private var recordedRetraction: RadrootsRetractionDraftInput?
  private var closed = false

  init(
    advanceOffline: Bool = false,
    saveFailure: RadrootsRuntimeFailure? = nil,
    includeWritableRelay: Bool = true,
    delayedPhase: AddDelayPhase? = nil,
    delayAfterBackgroundCompletion: Bool = false,
    schemas: [RadrootsAddSchema] = AddBackend.schemas()
  ) {
    self.advanceOffline = advanceOffline
    self.saveFailure = saveFailure
    self.includeWritableRelay = includeWritableRelay
    self.delayedPhase = delayedPhase
    self.delayAfterBackgroundCompletion = delayAfterBackgroundCompletion
    schemaInventory = schemas
  }

  func snapshot() -> RadrootsRuntimeSnapshot {
    RadrootsRuntimeSnapshot(
      identity: RadrootsRuntimeIdentity(
        publicKeyHex: String(repeating: "ab", count: 32),
        hostSignerConfigured: true
      ),
      relay: RadrootsRelayStatus(
        profile: "simulator",
        state: "configured",
        readAvailability: "unobserved",
        writeAvailability: "unobserved",
        relays: includeWritableRelay
          ? [
            RadrootsRelayEndpointStatus(
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
      blossomConfiguration: RadrootsBlossomConfigurationStatus(
        schemaVersion: 1,
        hostKind: "simulator",
        endpointAuthority: "loopback_development",
        primaryOrigin: "http://127.0.0.1:3000",
        fallbackOrigins: [],
        configFingerprint: String(repeating: "f", count: 64)
      ),
      blossomEvidence: nil,
      crateName: "radroots_mobile_ffi",
      crateVersion: "0.1.0-alpha",
      isClosed: closed
    )
  }

  func todayPage(request _: RadrootsTodayPageRequest) throws -> RadrootsTodayPage {
    throw unsupported()
  }

  func refreshToday(
    context _: RadrootsLocalNetwork,
    nowUnixSeconds _: UInt64,
    update _: RadrootsTodayProjectionUpdate
  ) throws -> RadrootsTodayRefreshReceipt {
    throw unsupported()
  }

  func addSchemas() -> [RadrootsAddSchema] {
    schemaInventory
  }

  nonisolated static func schemas() -> [RadrootsAddSchema] {
    let field = {
      (
        id: String,
        label: String,
        kind: RadrootsAddFieldKind,
        required: Bool,
        choices: [String],
        maxBytes: UInt64?,
        maxItems: UInt16?
      ) in
      RadrootsAddField(
        schemaVersion: 1,
        id: id,
        label: label,
        kind: kind,
        required: required,
        choices: choices,
        maxBytes: maxBytes,
        maxItems: maxItems
      )
    }
    let text = {
      (id: String, label: String, kind: RadrootsAddFieldKind, required: Bool, maximum: UInt64?) in
      field(id, label, kind, required, [], maximum, nil)
    }
    let media = { (required: Bool, maximum: UInt16) in
      field("media", "Photos", .media, required, [], 10 * 1024 * 1024, maximum)
    }
    return [
      RadrootsAddSchema(
        schemaVersion: 1,
        commandType: .createUpdate,
        label: "Update",
        fields: [text("content", "Update", .multilineText, true, 65535)]
      ),
      RadrootsAddSchema(
        schemaVersion: 1,
        commandType: .createPhotoUpdate,
        label: "Photo update",
        fields: [
          text("content", "Update", .multilineText, true, 65535),
          media(true, 20),
        ]
      ),
      RadrootsAddSchema(
        schemaVersion: 1,
        commandType: .createAsk,
        label: "Ask",
        fields: [
          text("content", "Question", .multilineText, true, 65535),
          media(false, 20),
        ]
      ),
      RadrootsAddSchema(
        schemaVersion: 1,
        commandType: .createEvent,
        label: "Event",
        fields: [
          text("identifier", "Identifier", .text, true, 256),
          text("title", "Title", .text, true, 256),
          text("content", "Description", .multilineText, false, 65535),
          text("event_start", "Starts", .dateTime, true, nil),
          text("event_end", "Ends", .dateTime, false, nil),
          text("location", "Location", .location, false, 256),
          media(false, 1),
        ]
      ),
      RadrootsAddSchema(
        schemaVersion: 1,
        commandType: .createFoodAvailability,
        label: "Food availability",
        fields: [
          text("identifier", "Identifier", .text, true, 256),
          text("title", "Food", .text, true, 256),
          text("summary", "Summary", .text, true, 256),
          text("content", "Details", .multilineText, true, 65535),
          text("location", "Pickup location", .location, true, 256),
          text("price_amount", "Price", .decimal, true, 64),
          field("currency", "Currency", .choice, true, [], 3, nil),
          field(
            "unit", "Unit", .choice, true,
            ["g", "kg", "lb", "oz", "each", "dozen", "bunch", "punnet", "bag", "basket"],
            nil, nil
          ),
          text("quantity", "Available quantity", .decimal, false, 64),
          media(false, 20),
        ]
      ),
    ]
  }

  func saveAddIntent(
    input: RadrootsAddRuntimeInput,
    existingDraftID: String?,
    expectedRevision: UInt64?
  ) async throws -> RadrootsDraftStatus {
    if let saveFailure {
      throw saveFailure
    }
    try await delayOnce(at: .save)
    let id = existingDraftID ?? String(format: "%032x", values.count + 1)
    let savedMedia = input.form.media.map {
      RadrootsPreparedMedia(
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
        RadrootsDraftMediaStatus(
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

  func probeBlossom() -> RadrootsBlossomEvidence {
    RadrootsBlossomEvidence(
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
    input: RadrootsRetractionDraftInput,
    authoredAtUnixSeconds _: UInt64,
    persistedAtUnixMilliseconds: UInt64
  ) -> RadrootsDraftStatus {
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

  func lastRetraction() -> RadrootsRetractionDraftInput? {
    recordedRetraction
  }

  func saveRevisionIntent(
    target: RadrootsRevisionTarget,
    replacement: RadrootsAddRuntimeInput
  ) -> RadrootsRevisionStatus {
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
    return RadrootsRevisionStatus(
      operationID: id,
      replacement: status,
      retraction: nil,
      policy: target.sourceAddress == nil ? .replaceThenRetract : .addressableReplacement,
      phase: .replacementPending
    )
  }

  func revisionStatus(operationID: String) throws -> RadrootsRevisionStatus {
    let replacement = try draftStatus(id: operationID)
    return RadrootsRevisionStatus(
      operationID: operationID,
      replacement: replacement,
      retraction: nil,
      policy: .addressableReplacement,
      phase: replacement.state == .complete ? .complete : .replacementPending
    )
  }

  func advanceRevision(operationID: String) throws -> RadrootsRevisionStatus {
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

  func cancelRevision(operationID: String) throws -> RadrootsRevisionStatus {
    let current = try draftStatus(id: operationID)
    let cancelled = replacing(
      current,
      revision: current.revision + 1,
      state: .cancelled,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[operationID] = cancelled
    return RadrootsRevisionStatus(
      operationID: operationID,
      replacement: cancelled,
      retraction: nil,
      policy: .addressableReplacement,
      phase: .cancelled
    )
  }

  func draftStatus(id: String) throws -> RadrootsDraftStatus {
    try storedDraft(id: id)
  }

  func draftHeads(limit: UInt16) -> [RadrootsDraftStatus] {
    Array(values.values.prefix(Int(limit)))
  }

  func queueAddIntent(
    id: String,
    expectedRevision: UInt64
  ) async throws -> RadrootsDraftStatus {
    try await delayOnce(at: .queue)
    guard includeWritableRelay else {
      throw RadrootsRuntimeFailure(
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

  func recoverAddIntent(id: String) throws -> RadrootsDraftStatus {
    let current = try draftStatus(id: id)
    let value = replacing(
      current, revision: current.revision + 1, state: .queued,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[id] = value
    return value
  }

  func uploadAddMediaIntent(input: RadrootsBlossomUploadIntent) throws -> RadrootsDraftStatus {
    uploadedMedia = true
    let current = try storedDraft(id: input.draftID)
    let verified = current.media.map {
      RadrootsDraftMediaStatus(
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
    input: RadrootsBlossomUploadIntent
  ) throws -> RadrootsNativeUploadJob {
    let current = try draftStatus(id: input.draftID)
    let uploading = replacing(
      current,
      revision: current.revision + 1,
      state: .mediaUploading,
      updatedAt: current.updatedAtUnixMilliseconds + 1
    )
    values[current.id] = uploading
    return RadrootsNativeUploadJob(
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
    input: RadrootsNativeUploadCompletion
  ) async throws -> RadrootsDraftStatus {
    uploadedMedia = true
    let current = try storedDraft(id: input.draftID)
    let verified = current.media.map {
      RadrootsDraftMediaStatus(
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

  func advanceDraft(id: String, expectedRevision: UInt64) async throws -> RadrootsDraftStatus {
    try await delayOnce(at: .advance)
    if advanceOffline {
      throw RadrootsRuntimeFailure(
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
  ) throws -> RadrootsDraftStatus {
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
    receive _: @escaping @Sendable (RadrootsRuntimeChange) async -> Void
  ) -> any RadrootsRuntimeSubscriptionToken {
    AddSubscriptionToken()
  }

  func shutdown() -> RadrootsRuntimeShutdownReceipt {
    let wasClosed = closed
    closed = true
    return RadrootsRuntimeShutdownReceipt(state: "closed", alreadyClosed: wasClosed)
  }

  private func makeStatus(
    id: String,
    revision: UInt64,
    kind: RadrootsDraftKind,
    commandType: RadrootsAddCommandType,
    form: RadrootsAddForm?,
    state: RadrootsOutboxState,
    updatedAt: UInt64,
    media: [RadrootsDraftMediaStatus],
    isRevision: Bool = false
  ) -> RadrootsDraftStatus {
    RadrootsDraftStatus(
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
    _ value: RadrootsDraftStatus,
    revision: UInt64,
    state: RadrootsOutboxState,
    updatedAt: UInt64,
    media: [RadrootsDraftMediaStatus]? = nil
  ) -> RadrootsDraftStatus {
    RadrootsDraftStatus(
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

  private func settlement() -> RadrootsOperationSettlement {
    RadrootsOperationSettlement(
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

  private func storedDraft(id: String) throws -> RadrootsDraftStatus {
    guard let value = values[id] else { throw unsupported() }
    return value
  }

  private func unsupported() -> RadrootsRuntimeFailure {
    .local(
      operation: "test.add", code: "test.unsupported", safeMessage: "Unsupported test operation."
    )
  }
}

private actor AddSubscriptionToken: RadrootsRuntimeSubscriptionToken {
  func cancel() {}
}

private final class BackgroundUploadFixture: @unchecked Sendable {
  let draftID = String(repeating: "1", count: 32)
  let media: RadrootsPreparedMedia
  private let root: URL
  private let roots: RadrootsAppleFileRoots

  init() throws {
    let bytes = Data("radroots-background-upload".utf8)
    let digest = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    root = FileManager.default.temporaryDirectory
      .appendingPathComponent("radroots-background-tests-\(UUID().uuidString)", isDirectory: true)
    roots = try RadrootsAppleFileRoots(
      appIdentifier: "org.radroots.background-tests",
      dataRoot: root.appendingPathComponent("data", isDirectory: true),
      cacheRoot: root.appendingPathComponent("cache", isDirectory: true),
      temporaryRoot: root.appendingPathComponent("temporary", isDirectory: true)
    )
    try FileManager.default.createDirectory(
      at: roots.stagedBlobsRoot,
      withIntermediateDirectories: true
    )
    try bytes.write(to: roots.stagedBlobsRoot.appendingPathComponent(digest))
    media = RadrootsPreparedMedia(
      opaqueReference: "media:\(digest)",
      remoteURL: "http://127.0.0.1:3000/\(digest).png",
      sha256: digest,
      mediaType: "image/png",
      byteSize: UInt64(bytes.count),
      width: 2,
      height: 2,
      alt: "Background upload",
      preparedAtUnixSeconds: 1_800_000_000
    )
  }

  func remove() {
    try? FileManager.default.removeItem(at: root)
  }

  func coordinator(transfer: any RadrootsBackgroundTransfer) -> RadrootsAddMediaCoordinator {
    RadrootsAddMediaCoordinator(
      roots: roots,
      picker: BackgroundMediaPicker(),
      preparer: RadrootsAppleMediaPreparer(roots: roots),
      transfer: transfer
    )
  }

  func job(revision: UInt64, operation: String) -> RadrootsNativeUploadJob {
    RadrootsNativeUploadJob(
      operationID: operation,
      draft: draft(revision: revision, stage: .uploading),
      remoteURL: media.remoteURL!,
      authorizationHeader: "Nostr test-authorization",
      expectedSHA256: media.sha256,
      mediaType: media.mediaType,
      byteSize: media.byteSize
    )
  }

  func draft(revision: UInt64, stage: RadrootsDraftMediaStage) -> RadrootsDraftStatus {
    var form = RadrootsAddForm.empty(.createPhotoUpdate)
    form.content = "Background transfer"
    form.media = [media]
    return RadrootsDraftStatus(
      id: draftID,
      revision: revision,
      authorPublicKey: String(repeating: "a", count: 64),
      kind: .add,
      commandType: .createPhotoUpdate,
      form: form,
      state: stage == .verified ? .readyToSign : .mediaUploading,
      cardID: String(repeating: "c", count: 64),
      operationID: String(repeating: "d", count: 32),
      createdAtUnixMilliseconds: 1_800_000_000_000,
      updatedAtUnixMilliseconds: 1_800_000_000_001,
      media: [
        RadrootsDraftMediaStatus(
          url: media.remoteURL!,
          stage: stage,
          uploadAttempts: stage == .verified ? 1 : 0,
          verifiedAtUnixMilliseconds: stage == .verified ? 1_800_000_000_001 : nil,
          possibleOrphan: false,
          orphanReasonCode: nil,
          orphanRecordedAtUnixMilliseconds: nil
        ),
      ],
      settlement: nil,
      isRevision: false
    )
  }

  func request(
    job: RadrootsNativeUploadJob,
    remoteURL: String? = nil
  ) throws -> RadrootsBackgroundTransferRequest {
    let blob = try RadrootsStagedBlobReference(
      blobID: media.sha256,
      sizeBytes: Int(media.byteSize),
      mediaType: media.mediaType,
      filenameHint: "\(media.sha256).png"
    )
    return try RadrootsBackgroundTransferRequest(
      identifier: RadrootsBackgroundTransferIdentifier(job.transferIdentifier),
      remoteURL: URL(string: remoteURL ?? job.remoteURL)!,
      method: .put,
      operation: .upload(source: .stagedBlob(blob)),
      headers: [:],
      metadata: [:],
      networkPolicy: .simulatorLoopbackHTTP,
      responsePolicy: .boundedJSON(),
      expectedSourceSHA256: media.sha256
    )
  }
}

extension RadrootsNativeUploadJob {
  fileprivate var transferIdentifier: String {
    "radroots.add.\(draft.id).\(draft.revision).\(operationID)"
  }
}

private struct BackgroundMediaPicker: RadrootsMediaPicker {
  func currentSupport() async throws -> RadrootsMediaPickerSupport {
    try RadrootsMediaPickerSupport(
      importAvailable: false,
      cameraCaptureAvailable: false,
      supportedImportKinds: [],
      supportedCaptureKinds: [],
      multipleSelectionSupported: false
    )
  }

  func importMedia(_: RadrootsMediaImportRequest) async throws -> RadrootsMediaImportResult {
    throw RadrootsCaptureIntakeError.unavailable
  }

  func captureMedia(_: RadrootsMediaCaptureRequest) async throws -> RadrootsMediaCaptureResult {
    throw RadrootsCaptureIntakeError.unavailable
  }
}

private actor BackgroundTransferHarness: RadrootsBackgroundTransfer {
  private var values: [RadrootsBackgroundTransferIdentifier: RadrootsBackgroundTransferSnapshot] =
    [:]
  private let enqueueState: RadrootsBackgroundTransferState
  private let pause: BackgroundTransferPause
  private var pauseReleased: Bool
  private(set) var isPaused = false
  private(set) var enqueueCount = 0
  private(set) var retryCount = 0
  private(set) var cancelCount = 0
  private(set) var acceptedSettlementCount = 0
  private(set) var snapshotCount = 0

  init(
    enqueueState: RadrootsBackgroundTransferState = .awaitingVerification,
    pause: BackgroundTransferPause = .none
  ) {
    self.enqueueState = enqueueState
    self.pause = pause
    pauseReleased = pause == .none
  }

  var state: RadrootsBackgroundTransferState? {
    values.values.first?.state
  }

  var counts: (enqueue: Int, retry: Int, cancel: Int, acceptedSettlement: Int) {
    (enqueueCount, retryCount, cancelCount, acceptedSettlementCount)
  }

  func seed(
    request: RadrootsBackgroundTransferRequest,
    state: RadrootsBackgroundTransferState
  ) throws {
    values[request.identifier] = try snapshot(request: request, state: state)
  }

  func setState(_ state: RadrootsBackgroundTransferState) throws {
    for (identifier, value) in values {
      values[identifier] = try snapshot(request: value.request, state: state)
    }
  }

  func removeAll() {
    values.removeAll()
  }

  func releasePause() {
    pauseReleased = true
  }

  func enqueue(_ request: RadrootsBackgroundTransferRequest) async throws
    -> RadrootsBackgroundTransferHandle
  {
    enqueueCount += 1
    values[request.identifier] = try snapshot(
      request: persisted(request),
      state: enqueueState
    )
    return RadrootsBackgroundTransferHandle(request: request)
  }

  func retry(_ request: RadrootsBackgroundTransferRequest) async throws
    -> RadrootsBackgroundTransferHandle
  {
    retryCount += 1
    values[request.identifier] = try snapshot(
      request: persisted(request),
      state: .awaitingVerification
    )
    return RadrootsBackgroundTransferHandle(request: request)
  }

  func cancel(_ identifier: RadrootsBackgroundTransferIdentifier) async throws {
    cancelCount += 1
    if let value = values[identifier] {
      values[identifier] = try snapshot(request: value.request, state: .cancelled)
    }
  }

  func expire(_ identifier: RadrootsBackgroundTransferIdentifier) async throws {
    if let value = values[identifier] {
      values[identifier] = try snapshot(request: value.request, state: .expired)
    }
  }

  func settle(
    _ identifier: RadrootsBackgroundTransferIdentifier,
    verification: RadrootsBackgroundTransferVerification
  ) async throws {
    guard let value = values[identifier] else {
      throw RadrootsBackgroundTransferError.transferFailure
    }
    switch verification {
    case .accepted:
      acceptedSettlementCount += 1
      values[identifier] = try snapshot(request: value.request, state: .completed)
    case let .rejected(failure):
      values[identifier] = try RadrootsBackgroundTransferSnapshot(
        request: value.request,
        state: .failed,
        failure: failure
      )
    }
  }

  func snapshot(for identifier: RadrootsBackgroundTransferIdentifier) async throws
    -> RadrootsBackgroundTransferSnapshot?
  {
    snapshotCount += 1
    try await waitIfPaused(at: .snapshot)
    return values[identifier]
  }

  func snapshots() async throws -> [RadrootsBackgroundTransferSnapshot] {
    try await waitIfPaused(at: .discovery)
    return values.values.sorted { $0.identifier < $1.identifier }
  }

  func handleEventsForBackgroundURLSession(
    identifier _: String,
    completionHandler: @escaping @Sendable () -> Void
  ) async {
    completionHandler()
  }

  private func persisted(
    _ request: RadrootsBackgroundTransferRequest
  ) throws -> RadrootsBackgroundTransferRequest {
    try RadrootsBackgroundTransferRequest(
      identifier: request.identifier,
      remoteURL: request.remoteURL,
      method: request.method,
      operation: request.operation,
      headers: [:],
      metadata: [:],
      networkPolicy: request.networkPolicy,
      responsePolicy: request.responsePolicy,
      expectedSourceSHA256: request.expectedSourceSHA256,
      maximumTransferBytes: request.maximumTransferBytes
    )
  }

  private func snapshot(
    request: RadrootsBackgroundTransferRequest,
    state: RadrootsBackgroundTransferState
  ) throws -> RadrootsBackgroundTransferSnapshot {
    try RadrootsBackgroundTransferSnapshot(
      request: request,
      state: state,
      response: [.awaitingVerification, .completed].contains(state)
        ? RadrootsBackgroundTransferResponse(
          statusCode: 200,
          mediaType: "application/json",
          body: Data("{}".utf8)
        ) : nil,
      possibleRemoteOrphan: false,
      updatedAt: Date(timeIntervalSince1970: 1_800_000_000)
    )
  }

  private func waitIfPaused(at point: BackgroundTransferPause) async throws {
    guard pause == point, !pauseReleased else { return }
    isPaused = true
    defer { isPaused = false }
    while !pauseReleased {
      try await Task.sleep(nanoseconds: 1_000_000)
    }
  }
}

private enum BackgroundTransferPause: Sendable {
  case none
  case discovery
  case snapshot
}
