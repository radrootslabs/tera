import Foundation
import RadrootsKit

struct TeraAddMediaSupport: Sendable, Equatable {
  let library: Bool
  let camera: Bool

  static let unavailable = Self(library: false, camera: false)
}

struct TeraAddBackgroundUploadReceipt: Sendable, Equatable {
  let identifier: String
  let draftID: String
  let expectedRevision: UInt64
  let statusCode: UInt16
  let mediaType: String?
  let contentEncoding: String?
  let body: Data
}

protocol TeraAddMediaHandling: Sendable {
  func renewSubmissionUpload(_ submission: TeraSubmissionStatus, media: TeraPreparedMedia, client: TeraRuntimeClient) async throws -> TeraSubmissionStatus
  func recoverNativeUploads(client: TeraRuntimeClient) async throws -> TeraNativeRecoveryProgress
  func recoverNativeUpload(key: String, client: TeraRuntimeClient) async throws -> TeraNativeRecoveryProgress
  func confirmDurableComposerMedia(_ media: [TeraComposerMedia]) async throws
  func prefersSharedForegroundUpload(ownerID: String) async throws -> Bool
  func support() async throws -> TeraAddMediaSupport
  func importImages(limit: Int) async throws -> [TeraPreparedMedia]
  func captureImage() async throws -> TeraPreparedMedia
  func open(_ media: [TeraPreparedMedia]) async throws -> TeraOpenedMedia
  func uploadInBackground(
    job: TeraNativeUploadJob,
    media: TeraPreparedMedia
  ) async throws -> TeraAddBackgroundUploadReceipt
  func uploadInBackground(transfer: TeraNativeTransferJob, media: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt
  func reconcileBackgroundSubmissions(_ submissions: [TeraSubmissionStatus], client: TeraRuntimeClient) async throws
  func retainedSubmissionUpload(_ submission: TeraSubmissionStatus, media: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt?
  func settleBackgroundUpload(identifier: String, accepted: Bool) async throws
  func reconcileBackgroundUploads(drafts: [TeraDraftStatus], client: TeraRuntimeClient) async throws
}

extension TeraAddMediaHandling {
  func recoverNativeUpload(key _: String, client _: TeraRuntimeClient) async throws -> TeraNativeRecoveryProgress {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func renewSubmissionUpload(_: TeraSubmissionStatus, media _: TeraPreparedMedia, client _: TeraRuntimeClient) async throws -> TeraSubmissionStatus {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func recoverNativeUploads(client _: TeraRuntimeClient) async throws -> TeraNativeRecoveryProgress {
    throw TeraComposerAcknowledgment.unconfirmed
  }

  func confirmDurableComposerMedia(_ media: [TeraComposerMedia]) async throws {
    guard media.isEmpty else { throw TeraComposerAcknowledgment.unconfirmed }
  }

  func prefersSharedForegroundUpload(ownerID _: String) async throws -> Bool {
    // A conformer must explicitly establish native capability before selecting it.
    // The shared uploader retains the required destination enforcement by default.
    true
  }

  func uploadInBackground(job: TeraNativeUploadJob, media: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt {
    try await uploadInBackground(transfer: job.transfer, media: media)
  }

  func uploadInBackground(transfer _: TeraNativeTransferJob, media _: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt {
    throw TeraRuntimeFailure.local(
      operation: "add.media.background", code: "ios.add.background_transfer_unavailable",
      safeMessage: "Background photo upload is unavailable on this device."
    )
  }

  func reconcileBackgroundSubmissions(_: [TeraSubmissionStatus], client: TeraRuntimeClient) async throws {
    _ = try? await recoverNativeUploads(client: client)
  }

  func retainedSubmissionUpload(_: TeraSubmissionStatus, media _: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt? {
    nil
  }

  func settleBackgroundUpload(identifier _: String, accepted _: Bool) async throws {}

  func reconcileBackgroundUploads(drafts _: [TeraDraftStatus], client: TeraRuntimeClient) async throws {
    _ = try? await recoverNativeUploads(client: client)
  }
}

actor TeraAddMediaCoordinator: TeraAddMediaHandling {
  private let roots: RadrootsAppleFileRoots
  private let picker: any RadrootsMediaPicker
  private let preparer: RadrootsAppleMediaPreparer
  private let transfer: any RadrootsBackgroundTransfer
  private let clock: TeraClock
  /// Reserve before request preparation or native callbacks. A cancelled waiter
  /// releases this caller's admission; OS transfer state remains authoritative.
  private var activeUploadDrafts: Set<String> = []
  private var recoveryActive = false

  func renewSubmissionUpload(_ submission: TeraSubmissionStatus, media: TeraPreparedMedia, client: TeraRuntimeClient) async throws -> TeraSubmissionStatus {
    guard activeUploadDrafts.insert(submission.intentID).inserted else { throw TeraBackgroundUploadRequest.operationInProgress }
    defer { activeUploadDrafts.remove(submission.intentID) }
    let roots = roots, transfer = transfer, preparer = preparer
    return try await client.withUploadRenewal { backend in
      let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
      defer { withExtendedLifetime(mediaUse) {} }
      let opened = try await self.open([media])
      defer { opened.close() }
      guard let handle = opened.handles.first else { throw TeraComposerAcknowledgment.unconfirmed }
      return try await TeraUploadRenewal(transfer: transfer, preparer: preparer)
        .run(submission, media: media, handle: handle, backend: backend)
    }
  }

  func confirmDurableComposerMedia(_ media: [TeraComposerMedia]) throws {
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    try TeraComposerMediaOwnership.confirm(media, roots: roots)
  }

  func prefersSharedForegroundUpload(ownerID: String) async throws -> Bool {
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    guard !RadrootsAppleBackgroundTransferAdapters.supportsNewEnqueue(for: .publicHTTPS) else { return false }
    // Preserve an existing native attempt for its own reconciliation path.
    // An uncertain native result never authorizes a second foreground PUT.
    let prefix = "radroots.add.\(ownerID)."
    return try await !transfer.snapshots().contains {
      $0.identifier.rawValue.hasPrefix(prefix) && $0.state != .completed
    }
  }

  init(
    roots: RadrootsAppleFileRoots,
    picker: any RadrootsMediaPicker,
    preparer: RadrootsAppleMediaPreparer,
    transfer: any RadrootsBackgroundTransfer,
    clock: TeraClock = .system
  ) {
    self.roots = roots
    self.picker = picker
    self.preparer = preparer
    self.transfer = transfer
    self.clock = clock
  }

  static func production(
    bundleIdentifier: String,
    transfer: any RadrootsBackgroundTransfer
  ) throws -> Self {
    let roots = try TeraRemoteQualificationEnvironment.applicationFileRoots(
      appIdentifier: bundleIdentifier
    )
    let fileAccess = RadrootsAppleFileAccess(roots: roots)
    let picker: any RadrootsMediaPicker
    #if DEBUG
      if let mediaFile = try TeraRemoteQualificationEnvironment.current()?.mediaFile {
        picker = TeraRemoteQualificationMediaPicker(
          roots: roots,
          file: mediaFile
        )
      } else {
        picker = RadrootsAppleMediaPicker(fileAccess: fileAccess)
      }
    #else
      picker = RadrootsAppleMediaPicker(fileAccess: fileAccess)
    #endif
    return Self(
      roots: roots,
      picker: picker,
      preparer: RadrootsAppleMediaPreparer(roots: roots),
      transfer: transfer
    )
  }

  func support() async throws -> TeraAddMediaSupport {
    let value = try await picker.currentSupport()
    return TeraAddMediaSupport(
      library: value.importAvailable && value.supportedImportKinds.contains(.image),
      camera: value.cameraCaptureAvailable && value.supportedCaptureKinds.contains(.image)
    )
  }

  func importImages(limit: Int) async throws -> [TeraPreparedMedia] {
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    let result = try await picker.importMedia(
      RadrootsMediaImportRequest(
        allowedMediaKinds: [.image],
        selectionLimit: min(max(limit, 1), 20),
        destinationScope: .cache
      )
    )
    var prepared: [TeraPreparedMedia] = []
    for asset in result.items {
      try await prepared.append(prepare(asset))
    }
    return prepared
  }

  func captureImage() async throws -> TeraPreparedMedia {
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    let result = try await picker.captureMedia(
      RadrootsMediaCaptureRequest(mediaKind: .image, destinationScope: .cache)
    )
    return try await prepare(result.item)
  }

  func open(_ media: [TeraPreparedMedia]) throws -> TeraOpenedMedia {
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    var files: [FileHandle] = []
    var handles: [TeraPreparedMediaHandle] = []
    do {
      for item in media {
        guard item.opaqueReference == "media:\(item.sha256)",
          item.mediaType == "image/png",
          let byteSize = Int(exactly: item.byteSize)
        else {
          throw TeraRuntimeFailure.local(
            operation: "add.media.open",
            code: "ios.add.media_reference_invalid",
            safeMessage: "A prepared photo is no longer available."
          )
        }
        let blob = try RadrootsStagedBlobReference(
          blobID: item.sha256,
          sizeBytes: byteSize,
          mediaType: item.mediaType,
          filenameHint: "\(item.sha256).png"
        )
        try TeraDurableMediaRoots.restoreLegacyBlob(blob, roots: roots)
        let file = try FileHandle(forReadingFrom: roots.stagedBlobURL(for: blob))
        files.append(file)
        try handles.append(
          TeraPreparedMediaHandle(
            media: item,
            fileDescriptor: UInt64(file.fileDescriptor)
          )
        )
      }
      return TeraOpenedMedia(handles: handles, files: files)
    } catch {
      for file in files {
        try? file.close()
      }
      throw error
    }
  }

  func uploadInBackground(
    transfer job: TeraNativeTransferJob,
    media: TeraPreparedMedia
  ) async throws -> TeraAddBackgroundUploadReceipt {
    try Task.checkCancellation()
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    guard activeUploadDrafts.insert(job.ownerID).inserted else {
      throw TeraBackgroundUploadRequest.operationInProgress
    }
    defer { activeUploadDrafts.remove(job.ownerID) }
    let request = try await TeraBackgroundUploadRequest.prepare(
      job: job, media: media, preparer: preparer
    )
    let persisted = try await TeraBackgroundUploadWaiter.matchingPersistedUpload(transfer: transfer,
                                                                                 draftID: job.ownerID,
                                                                                 expectedRevision: job.expectedRevision,
                                                                                 request: request)
    let active: RadrootsBackgroundTransferSnapshot
    if let persisted {
      if [.failed, .interrupted, .cancelled, .expired].contains(persisted.state) {
        let retry = try TeraBackgroundUploadRequest.replacingIdentifier(in: request, with: persisted.identifier)
        try Task.checkCancellation()
        active = try await TeraNativeUploadExecution.start(transfer: transfer, request: retry, retrying: true)
      } else {
        active = persisted
      }
    } else {
      try Task.checkCancellation()
      active = try await TeraNativeUploadExecution.start(transfer: transfer, request: request, retrying: false)
    }
    return try await TeraBackgroundUploadWaiter.receipt(transfer: transfer,
                                                        for: active.identifier,
                                                        draftID: job.ownerID,
                                                        expectedRevision: job.expectedRevision,
                                                        request: request, baseline: active)
  }

  func settleBackgroundUpload(identifier: String, accepted: Bool) async throws {
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    do {
      let value = try RadrootsBackgroundTransferIdentifier(identifier)
      if let snapshot = try await transfer.snapshot(for: value),
        snapshot.state == .completed, accepted
      {
        return
      }
      try await transfer.settle(
        value,
        verification: accepted
          ? .accepted
          : .rejected(failure: .verificationRejected)
      )
    } catch is CancellationError {
      throw CancellationError()
    } catch {
      throw TeraRuntimeFailure.local(
        operation: "add.media.background.settle",
        code: "ios.add.background_settlement_failed",
        safeMessage: "The verified photo transfer could not be finalized."
      )
    }
  }

  func retainedSubmissionUpload(_ submission: TeraSubmissionStatus, media: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt? {
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    return try await TeraStoppedUploadRecovery.receipt(submission, media: media, transfer: transfer)
  }

  private func prepare(_ asset: RadrootsMediaAsset) async throws -> TeraPreparedMedia {
    let prepared = try await preparer.prepareImage(
      RadrootsAppleImagePreparationRequest(source: .file(asset.file))
    )
    return try TeraPreparedMedia(
      opaqueReference: "media:\(prepared.sha256)",
      remoteURL: nil,
      sha256: prepared.sha256,
      mediaType: "image/png",
      byteSize: UInt64(prepared.file.sizeBytes),
      width: prepared.width,
      height: prepared.height,
      alt: "",
      preparedAtUnixSeconds: clock.unixSeconds()
    )
  }

  private static func failure(code: String, message: String) -> TeraRuntimeFailure {
    .local(operation: "add.media.background", code: code, safeMessage: message)
  }
}

extension TeraAddMediaCoordinator {
  func recoverNativeUploads(client: TeraRuntimeClient) async throws -> TeraNativeRecoveryProgress {
    try await recoverNativeUploads(selectedKey: nil, client: client)
  }

  func recoverNativeUpload(key: String, client: TeraRuntimeClient) async throws -> TeraNativeRecoveryProgress {
    guard key.utf8.count == 64, key.utf8.allSatisfy({ (48 ... 57).contains($0) || (97 ... 102).contains($0) }) else {
      throw TeraComposerAcknowledgment.unconfirmed
    }
    return try await recoverNativeUploads(selectedKey: key, client: client)
  }

  private func recoverNativeUploads(selectedKey: String?, client: TeraRuntimeClient) async throws -> TeraNativeRecoveryProgress {
    guard !recoveryActive else { throw TeraBackgroundUploadRequest.operationInProgress }
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    recoveryActive = true
    defer { recoveryActive = false; withExtendedLifetime(mediaUse) {} }
    let schedule = selectedKey == nil ? try await client.nativeRecoverySchedule() : nil
    let continuation = schedule.map { TeraNativeRecoveryContinuation($0, client: client) }
    let inspection = TeraNativeRecoveryInspection(selectedKey: selectedKey, completedNeedsRepair: { key in
      guard let status = try await client.nativeRecoveryStatus(key: key) else { return false }
      return status.reason != .resolved
    })
    let result = try await TeraNativeRecoveryInventory.run(transfer: transfer, cursor: schedule?.after, inspection: inspection, checkpoint: { key in
      try await continuation?.advance(key)
    }, complete: { snapshot, owner in
      let input = try TeraRecoveryUploadReceipt(snapshot: snapshot, owner: owner)
      let opened = try await self.open([input.media])
      defer { opened.close() }
      guard let handle = opened.handles.first else { throw TeraComposerAcknowledgment.unconfirmed }
      let receipt = try await client.recoverNativeUpload(input, media: handle)
      try Task.checkCancellation()
      try receipt.confirm(input)
      try await self.settleRecoveredUpload(snapshot, input: input, receipt: receipt)
    }, report: { snapshot, reason in
      await TeraNativeRecoveryClassification.report(snapshot.identifier.rawValue, reason: reason, client: client)
    }, lookup: { key in try await client.recoveryUploadOwner(key: key) })
    return result.progress
  }

  private func settleRecoveredUpload(_ snapshot: RadrootsBackgroundTransferSnapshot, input: TeraRecoveryUploadReceipt,
                                     receipt: TeraRecoveryCompletionReceipt) async throws
  {
    try await TeraNativeUploadReconciliation.settle(snapshot, input: input, receipt: receipt, transfer: transfer)
  }
}
