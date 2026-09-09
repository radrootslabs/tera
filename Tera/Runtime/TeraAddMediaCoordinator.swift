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
  func support() async throws -> TeraAddMediaSupport
  func importImages(limit: Int) async throws -> [TeraPreparedMedia]
  func captureImage() async throws -> TeraPreparedMedia
  func open(_ media: [TeraPreparedMedia]) async throws -> TeraOpenedMedia
  func uploadInBackground(
    job: TeraNativeUploadJob,
    media: TeraPreparedMedia
  ) async throws -> TeraAddBackgroundUploadReceipt
  func settleBackgroundUpload(identifier: String, accepted: Bool) async throws
  func reconcileBackgroundUploads(drafts: [TeraDraftStatus]) async throws
}

extension TeraAddMediaHandling {
  func uploadInBackground(
    job _: TeraNativeUploadJob,
    media _: TeraPreparedMedia
  ) async throws -> TeraAddBackgroundUploadReceipt {
    throw TeraRuntimeFailure.local(
      operation: "add.media.background",
      code: "ios.add.background_transfer_unavailable",
      safeMessage: "Background photo upload is unavailable on this device."
    )
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
    let result = try await picker.captureMedia(
      RadrootsMediaCaptureRequest(mediaKind: .image, destinationScope: .cache)
    )
    return try await prepare(result.item)
  }

  func open(_ media: [TeraPreparedMedia]) throws -> TeraOpenedMedia {
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
    job: TeraNativeUploadJob,
    media: TeraPreparedMedia
  ) async throws -> TeraAddBackgroundUploadReceipt {
    try Task.checkCancellation()
    guard activeUploadDrafts.insert(job.draft.id).inserted else {
      throw TeraBackgroundUploadRequest.operationInProgress
    }
    defer { activeUploadDrafts.remove(job.draft.id) }
    let request = try await TeraBackgroundUploadRequest.prepare(
      job: job, media: media, preparer: preparer
    )
    let identifier = request.identifier
    let persisted = try await matchingPersistedUpload(
      draftID: job.draft.id,
      expectedRevision: job.draft.revision,
      request: request
    )
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
    return try await receipt(
      for: activeIdentifier,
      draftID: job.draft.id,
      expectedRevision: job.draft.revision,
      request: request
    )
  }

  func settleBackgroundUpload(identifier: String, accepted: Bool) async throws {
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
    var draftsByID: [String: TeraDraftStatus] = [:]
    for draft in drafts {
      guard draftsByID.updateValue(draft, forKey: draft.id) == nil else {
        throw Self.failure(
          code: "ios.add.background_draft_ambiguous",
          message: "The persisted draft inventory is ambiguous."
        )
      }
    }
    for snapshot in try await transfer.snapshots()
    where snapshot.state == .awaitingVerification {
      try Task.checkCancellation()
      guard let identity = TeraBackgroundUploadRequest.transferIdentity(snapshot.identifier),
        let draft = draftsByID[identity.draftID]
      else { continue }
      guard draft.revision > identity.revision,
        let form = draft.form,
        let media = form.media.first(where: {
          $0.remoteURL == snapshot.request.remoteURL.absoluteString
            && $0.sha256 == snapshot.request.expectedSourceSHA256
        }),
        draft.media.contains(where: { $0.url == media.remoteURL && $0.stage == .verified }),
        try TeraBackgroundUploadRequest.persistedRequestMatchesMedia(snapshot.request, media: media)
      else {
        throw Self.failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload does not match its verified draft."
        )
      }
      try await transfer.settle(snapshot.identifier, verification: .accepted)
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
      alt: "Farm photo",
      preparedAtUnixSeconds: clock.unixSeconds()
    )
  }

  private func matchingPersistedUpload(
    draftID: String,
    expectedRevision: UInt64,
    request: RadrootsBackgroundTransferRequest
  ) async throws -> RadrootsBackgroundTransferSnapshot? {
    try Task.checkCancellation()
    let snapshots = try await transfer.snapshots()
    let prefix = "radroots.add.\(draftID)."
    let owned = snapshots.filter { $0.identifier.rawValue.hasPrefix(prefix) }
    let parsed = try owned.map { snapshot in
      guard let identity = TeraBackgroundUploadRequest.transferIdentity(snapshot.identifier),
        identity.draftID == draftID,
        identity.revision <= expectedRevision
      else {
        throw Self.failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload identity is invalid."
        )
      }
      return snapshot
    }
    let candidates = parsed.filter { snapshot in
      snapshot.state != .completed
        || TeraBackgroundUploadRequest.persistedRequestMatches(snapshot.request, request: request)
    }
    try Task.checkCancellation()
    let active = candidates.filter { $0.state != .completed }
    guard active.count <= 1 else {
      throw Self.failure(
        code: "ios.add.background_upload_ambiguous",
        message: "The persisted photo upload state is ambiguous."
      )
    }
    if let candidate = active.first {
      guard TeraBackgroundUploadRequest.persistedRequestMatches(candidate.request, request: request) else {
        throw Self.failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload does not match the authorized upload."
        )
      }
      return candidate
    }
    let completed = candidates.filter {
      $0.state == .completed && TeraBackgroundUploadRequest.persistedRequestMatches($0.request, request: request)
    }
    guard completed.count <= 1 else {
      throw Self.failure(
        code: "ios.add.background_upload_ambiguous",
        message: "The persisted photo upload state is ambiguous."
      )
    }
    return completed.first
  }

  private func receipt(
    for identifier: RadrootsBackgroundTransferIdentifier,
    draftID: String,
    expectedRevision: UInt64,
    request: RadrootsBackgroundTransferRequest
  ) async throws -> TeraAddBackgroundUploadReceipt {
    while true {
      try Task.checkCancellation()
      guard let snapshot = try await transfer.snapshot(for: identifier) else {
        throw Self.failure(
          code: "ios.add.background_upload_missing",
          message: "The background photo upload could not be recovered."
        )
      }
      guard TeraBackgroundUploadRequest.persistedRequestMatches(snapshot.request, request: request) else {
        throw Self.failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload no longer matches its request."
        )
      }
      switch snapshot.state {
      case .awaitingVerification, .completed:
        try Task.checkCancellation()
        guard let response = snapshot.response,
          let statusCode = UInt16(exactly: response.statusCode),
          let body = response.body
        else {
          throw Self.failure(
            code: "ios.add.background_response_invalid",
            message: "The photo service returned an invalid response."
          )
        }
        return TeraAddBackgroundUploadReceipt(
          identifier: identifier.rawValue,
          draftID: draftID,
          expectedRevision: expectedRevision,
          statusCode: statusCode,
          mediaType: response.mediaType,
          contentEncoding: response.contentEncoding,
          body: body
        )
      case .failed, .interrupted, .cancelled, .expired:
        throw Self.failure(
          code: snapshot.failure?.rawValue ?? "ios.add.background_upload_failed",
          message: TeraUserMessages.text(.backgroundTransferFailed)
        )
      case .queued, .running:
        try await Task.sleep(for: .milliseconds(100))
      }
    }
  }

  private static func failure(code: String, message: String) -> TeraRuntimeFailure {
    .local(operation: "add.media.background", code: code, safeMessage: message)
  }
}

#if DEBUG
  private actor TeraRemoteQualificationMediaPicker: RadrootsMediaPicker {
    private let roots: RadrootsAppleFileRoots
    private let file: RadrootsFileReference

    init(roots: RadrootsAppleFileRoots, file: RadrootsFileReference) {
      self.roots = roots
      self.file = file
    }

    func currentSupport() async throws -> RadrootsMediaPickerSupport {
      try RadrootsMediaPickerSupport(
        importAvailable: true,
        cameraCaptureAvailable: false,
        supportedImportKinds: [.image],
        supportedCaptureKinds: [],
        multipleSelectionSupported: false
      )
    }

    func importMedia(
      _ request: RadrootsMediaImportRequest
    ) async throws -> RadrootsMediaImportResult {
      guard request.allowedMediaKinds == [.image], request.selectionLimit >= 1 else {
        throw RadrootsCaptureIntakeError.invalidRequest
      }
      try RadrootsAppleFileAccess(roots: roots).write(
        .inline(TeraRemoteQualificationEnvironment.mediaFixtureData()),
        to: file
      )
      let url = try roots.resolvedURL(for: file)
      let values = try url.resourceValues(
        forKeys: [.fileSizeKey, .isRegularFileKey, .isSymbolicLinkKey]
      )
      guard values.isRegularFile == true,
        values.isSymbolicLink != true,
        let size = values.fileSize,
        (1 ... 40 * 1024 * 1024).contains(size)
      else {
        throw RadrootsCaptureIntakeError.unavailable
      }
      let asset = try RadrootsMediaAsset(
        source: .libraryImport,
        kind: .image,
        file: file,
        mediaType: "image/png",
        suggestedFilename: "input.png",
        sizeBytes: UInt64(size),
        capturedAt: Date()
      )
      return try RadrootsMediaImportResult(items: [asset])
    }

    func captureMedia(
      _: RadrootsMediaCaptureRequest
    ) async throws -> RadrootsMediaCaptureResult {
      throw RadrootsCaptureIntakeError.unavailable
    }
  }
#endif
