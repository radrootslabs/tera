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
  func reconcileBackgroundSubmissions(_ submissions: [TeraSubmissionStatus]) async throws
  func retainedSubmissionUpload(_ submission: TeraSubmissionStatus, media: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt?
  func settleBackgroundUpload(identifier: String, accepted: Bool) async throws
  func reconcileBackgroundUploads(drafts: [TeraDraftStatus]) async throws
}

extension TeraAddMediaHandling {
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

  func reconcileBackgroundSubmissions(_: [TeraSubmissionStatus]) async throws {}

  func retainedSubmissionUpload(_: TeraSubmissionStatus, media _: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt? {
    nil
  }

  func settleBackgroundUpload(identifier _: String, accepted _: Bool) async throws {}

  func reconcileBackgroundUploads(drafts _: [TeraDraftStatus]) async throws {}
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
    let identifier = request.identifier
    let persisted = try await TeraBackgroundUploadWaiter.matchingPersistedUpload(transfer: transfer,
                                                                                 draftID: job.ownerID,
                                                                                 expectedRevision: job.expectedRevision,
                                                                                 request: request)
    let activeIdentifier: RadrootsBackgroundTransferIdentifier
    if let persisted {
      activeIdentifier = persisted.identifier
      if [.failed, .interrupted, .cancelled, .expired].contains(persisted.state) {
        let retry = try TeraBackgroundUploadRequest.replacingIdentifier(in: request, with: activeIdentifier)
        try Task.checkCancellation()
        _ = try await transfer.retry(retry)
      }
    } else {
      try Task.checkCancellation()
      _ = try await transfer.enqueue(request)
      activeIdentifier = identifier
    }
    return try await TeraBackgroundUploadWaiter.receipt(transfer: transfer,
                                                        for: activeIdentifier,
                                                        draftID: job.ownerID,
                                                        expectedRevision: job.expectedRevision,
                                                        request: request)
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

  func reconcileBackgroundUploads(drafts: [TeraDraftStatus]) async throws {
    try await reconcile(drafts.map { TeraNativeUploadRecoveryOwner(
      id: $0.id, revision: $0.revision, media: $0.form?.media ?? [],
      verifiedURLs: Set($0.media.filter { $0.stage == .verified }.map(\.url)),
      uploadURLs: uploadURLs($0.media)
    ) })
  }

  func reconcileBackgroundSubmissions(_ submissions: [TeraSubmissionStatus]) async throws {
    try await reconcile(submissions.map { TeraNativeUploadRecoveryOwner(
      id: $0.intentID, revision: $0.revision, media: $0.preparedMedia,
      verifiedURLs: Set($0.media.filter { $0.progress.stage == .verified }.map(\.progress.url)),
      uploadURLs: uploadURLs($0.media.map(\.progress))
    ) })
  }

  func retainedSubmissionUpload(_ submission: TeraSubmissionStatus, media: TeraPreparedMedia) async throws -> TeraAddBackgroundUploadReceipt? {
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    return try await TeraStoppedUploadRecovery.receipt(submission, media: media, transfer: transfer)
  }

  private func reconcile(_ owners: [TeraNativeUploadRecoveryOwner]) async throws {
    let mediaUse = try TeraMediaProcessUse.admit(root: roots.dataRoot)
    defer { withExtendedLifetime(mediaUse) {} }
    try await TeraNativeUploadReconciliation.reconcile(owners, transfer: transfer)
  }

  private func uploadURLs(_ media: [TeraDraftMediaStatus]) -> [String: String] {
    media.reduce(into: [:]) { urls, item in
      if let uploadURL = item.uploadURL {
        urls[item.url] = uploadURL
      }
    }
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
