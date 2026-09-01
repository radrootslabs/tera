import Foundation
import RadrootsKit

struct RadrootsAddMediaSupport: Sendable, Equatable {
  let library: Bool
  let camera: Bool

  static let unavailable = Self(library: false, camera: false)
}

struct RadrootsAddBackgroundUploadReceipt: Sendable, Equatable {
  let identifier: String
  let draftID: String
  let expectedRevision: UInt64
  let statusCode: UInt16
  let mediaType: String?
  let contentEncoding: String?
  let body: Data
}

protocol RadrootsAddMediaHandling: Sendable {
  func support() async throws -> RadrootsAddMediaSupport
  func importImages(limit: Int) async throws -> [RadrootsPreparedMedia]
  func captureImage() async throws -> RadrootsPreparedMedia
  func open(_ media: [RadrootsPreparedMedia]) async throws -> RadrootsOpenedMedia
  func uploadInBackground(
    job: RadrootsNativeUploadJob,
    media: RadrootsPreparedMedia
  ) async throws -> RadrootsAddBackgroundUploadReceipt
  func settleBackgroundUpload(identifier: String, accepted: Bool) async throws
  func reconcileBackgroundUploads(drafts: [RadrootsDraftStatus]) async throws
}

extension RadrootsAddMediaHandling {
  func uploadInBackground(
    job _: RadrootsNativeUploadJob,
    media _: RadrootsPreparedMedia
  ) async throws -> RadrootsAddBackgroundUploadReceipt {
    throw RadrootsRuntimeFailure.local(
      operation: "add.media.background",
      code: "ios.add.background_transfer_unavailable",
      safeMessage: "Background photo upload is unavailable on this device."
    )
  }

  func settleBackgroundUpload(identifier _: String, accepted _: Bool) async throws {}

  func reconcileBackgroundUploads(drafts _: [RadrootsDraftStatus]) async throws {}
}

final class RadrootsOpenedMedia: @unchecked Sendable {
  let handles: [RadrootsPreparedMediaHandle]
  private var files: [FileHandle]

  init(handles: [RadrootsPreparedMediaHandle], files: [FileHandle]) {
    self.handles = handles
    self.files = files
  }

  deinit {
    close()
  }

  func close() {
    let active = files
    files.removeAll()
    for file in active {
      try? file.close()
    }
  }
}

actor RadrootsAddMediaCoordinator: RadrootsAddMediaHandling {
  private let roots: RadrootsAppleFileRoots
  private let picker: any RadrootsMediaPicker
  private let preparer: RadrootsAppleMediaPreparer
  private let transfer: any RadrootsBackgroundTransfer

  init(
    roots: RadrootsAppleFileRoots,
    picker: any RadrootsMediaPicker,
    preparer: RadrootsAppleMediaPreparer,
    transfer: any RadrootsBackgroundTransfer
  ) {
    self.roots = roots
    self.picker = picker
    self.preparer = preparer
    self.transfer = transfer
  }

  static func production(
    bundleIdentifier: String,
    transfer: any RadrootsBackgroundTransfer
  ) throws -> Self {
    let roots = try RadrootsRemoteQualificationEnvironment.applicationFileRoots(
      appIdentifier: bundleIdentifier
    )
    let fileAccess = RadrootsAppleFileAccess(roots: roots)
    let picker: any RadrootsMediaPicker
    #if DEBUG
      if let mediaFile = try RadrootsRemoteQualificationEnvironment.current()?.mediaFile {
        picker = RadrootsRemoteQualificationMediaPicker(
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

  func support() async throws -> RadrootsAddMediaSupport {
    let value = try await picker.currentSupport()
    return RadrootsAddMediaSupport(
      library: value.importAvailable && value.supportedImportKinds.contains(.image),
      camera: value.cameraCaptureAvailable && value.supportedCaptureKinds.contains(.image)
    )
  }

  func importImages(limit: Int) async throws -> [RadrootsPreparedMedia] {
    let result = try await picker.importMedia(
      RadrootsMediaImportRequest(
        allowedMediaKinds: [.image],
        selectionLimit: min(max(limit, 1), 20),
        destinationScope: .cache
      )
    )
    var prepared: [RadrootsPreparedMedia] = []
    for asset in result.items {
      try await prepared.append(prepare(asset))
    }
    return prepared
  }

  func captureImage() async throws -> RadrootsPreparedMedia {
    let result = try await picker.captureMedia(
      RadrootsMediaCaptureRequest(mediaKind: .image, destinationScope: .cache)
    )
    return try await prepare(result.item)
  }

  func open(_ media: [RadrootsPreparedMedia]) throws -> RadrootsOpenedMedia {
    var files: [FileHandle] = []
    var handles: [RadrootsPreparedMediaHandle] = []
    do {
      for item in media {
        guard item.opaqueReference == "media:\(item.sha256)",
          item.mediaType == "image/png",
          let byteSize = Int(exactly: item.byteSize)
        else {
          throw RadrootsRuntimeFailure.local(
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
        handles.append(
          RadrootsPreparedMediaHandle(
            media: item,
            fileDescriptor: UInt64(file.fileDescriptor)
          )
        )
      }
      return RadrootsOpenedMedia(handles: handles, files: files)
    } catch {
      for file in files {
        try? file.close()
      }
      throw error
    }
  }

  func uploadInBackground(
    job: RadrootsNativeUploadJob,
    media: RadrootsPreparedMedia
  ) async throws -> RadrootsAddBackgroundUploadReceipt {
    guard job.expectedSHA256 == media.sha256,
      job.mediaType == media.mediaType,
      job.byteSize == media.byteSize,
      let byteSize = Int(exactly: media.byteSize),
      let remoteURL = URL(string: job.remoteURL)
    else {
      throw RadrootsRuntimeFailure.local(
        operation: "add.media.background",
        code: "ios.add.background_upload_mismatch",
        safeMessage: "The prepared photo no longer matches the authorized upload."
      )
    }
    let identifier = try Self.transferIdentifier(job: job)
    let prepared = try RadrootsApplePreparedImage(
      file: RadrootsStagedBlobReference(
        blobID: media.sha256,
        sizeBytes: byteSize,
        mediaType: media.mediaType,
        filenameHint: "\(media.sha256).png"
      ),
      sha256: media.sha256,
      width: media.width,
      height: media.height
    )
    let request = try await preparer.blossomUploadRequest(
      preparedImage: prepared,
      remoteURL: remoteURL,
      authorization: job.authorizationHeader,
      networkPolicy: remoteURL.scheme?.lowercased() == "https"
        ? .publicHTTPS : .simulatorLoopbackHTTP,
      identifier: identifier
    )
    let persisted = try await matchingPersistedUpload(
      draftID: job.draft.id,
      expectedRevision: job.draft.revision,
      request: request
    )
    let activeIdentifier: RadrootsBackgroundTransferIdentifier
    if let persisted {
      activeIdentifier = persisted.identifier
      if [.failed, .interrupted, .cancelled, .expired].contains(persisted.state) {
        let retry = try Self.replacingIdentifier(in: request, with: activeIdentifier)
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
      throw RadrootsRuntimeFailure.local(
        operation: "add.media.background.settle",
        code: "ios.add.background_settlement_failed",
        safeMessage: "The verified photo transfer could not be finalized."
      )
    }
  }

  func reconcileBackgroundUploads(drafts: [RadrootsDraftStatus]) async throws {
    var draftsByID: [String: RadrootsDraftStatus] = [:]
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
      guard let identity = Self.transferIdentity(snapshot.identifier),
        let draft = draftsByID[identity.draftID]
      else { continue }
      guard draft.revision > identity.revision,
        let form = draft.form,
        let media = form.media.first(where: {
          $0.remoteURL == snapshot.request.remoteURL.absoluteString
            && $0.sha256 == snapshot.request.expectedSourceSHA256
        }),
        draft.media.contains(where: { $0.url == media.remoteURL && $0.stage == .verified }),
        try Self.persistedRequestMatchesMedia(snapshot.request, media: media)
      else {
        throw Self.failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload does not match its verified draft."
        )
      }
      try await transfer.settle(snapshot.identifier, verification: .accepted)
    }
  }

  private func prepare(_ asset: RadrootsMediaAsset) async throws -> RadrootsPreparedMedia {
    let prepared = try await preparer.prepareImage(
      RadrootsAppleImagePreparationRequest(source: .file(asset.file))
    )
    return RadrootsPreparedMedia(
      opaqueReference: "media:\(prepared.sha256)",
      remoteURL: nil,
      sha256: prepared.sha256,
      mediaType: "image/png",
      byteSize: UInt64(prepared.file.sizeBytes),
      width: prepared.width,
      height: prepared.height,
      alt: "Farm photo",
      preparedAtUnixSeconds: UInt64(Date().timeIntervalSince1970)
    )
  }

  private static func transferIdentifier(
    job: RadrootsNativeUploadJob
  ) throws -> RadrootsBackgroundTransferIdentifier {
    try RadrootsBackgroundTransferIdentifier(
      "radroots.add.\(job.draft.id).\(job.draft.revision).\(job.operationID)"
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
      guard let identity = Self.transferIdentity(snapshot.identifier),
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
        || Self.persistedRequestMatches(snapshot.request, request: request)
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
      guard Self.persistedRequestMatches(candidate.request, request: request) else {
        throw Self.failure(
          code: "ios.add.background_upload_mismatch",
          message: "The persisted photo upload does not match the authorized upload."
        )
      }
      return candidate
    }
    let completed = candidates.filter {
      $0.state == .completed && Self.persistedRequestMatches($0.request, request: request)
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
  ) async throws -> RadrootsAddBackgroundUploadReceipt {
    while true {
      try Task.checkCancellation()
      guard let snapshot = try await transfer.snapshot(for: identifier) else {
        throw Self.failure(
          code: "ios.add.background_upload_missing",
          message: "The background photo upload could not be recovered."
        )
      }
      guard Self.persistedRequestMatches(snapshot.request, request: request) else {
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
        return RadrootsAddBackgroundUploadReceipt(
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
          message: RadrootsUserMessages.text(.backgroundTransferFailed)
        )
      case .queued, .running:
        try await Task.sleep(for: .milliseconds(100))
      }
    }
  }

  private static func persistedRequestMatches(
    _ persisted: RadrootsBackgroundTransferRequest,
    request: RadrootsBackgroundTransferRequest
  ) -> Bool {
    persisted.headers.isEmpty && persisted.metadata.isEmpty
      && persisted.remoteURL == request.remoteURL
      && persisted.method == request.method
      && persisted.operation == request.operation
      && persisted.networkPolicy == request.networkPolicy
      && persisted.responsePolicy == request.responsePolicy
      && persisted.expectedSourceSHA256 == request.expectedSourceSHA256
      && persisted.maximumTransferBytes == request.maximumTransferBytes
  }

  private static func persistedRequestMatchesMedia(
    _ persisted: RadrootsBackgroundTransferRequest,
    media: RadrootsPreparedMedia
  ) throws -> Bool {
    guard let remoteURL = media.remoteURL.flatMap(URL.init(string:)),
      let byteSize = Int(exactly: media.byteSize)
    else { return false }
    let blob = try RadrootsStagedBlobReference(
      blobID: media.sha256,
      sizeBytes: byteSize,
      mediaType: media.mediaType,
      filenameHint: "\(media.sha256).png"
    )
    let responsePolicy = try RadrootsBackgroundTransferResponsePolicy.boundedJSON()
    return persisted.headers.isEmpty && persisted.metadata.isEmpty
      && persisted.remoteURL == remoteURL
      && persisted.method == .put
      && persisted.operation == .upload(source: .stagedBlob(blob))
      && persisted.networkPolicy
        == (remoteURL.scheme?.lowercased() == "https" ? .publicHTTPS : .simulatorLoopbackHTTP)
      && persisted.responsePolicy == responsePolicy
      && persisted.expectedSourceSHA256 == media.sha256
      && persisted.maximumTransferBytes
        == RadrootsBackgroundTransferRequest.defaultMaximumTransferBytes
  }

  private static func replacingIdentifier(
    in request: RadrootsBackgroundTransferRequest,
    with identifier: RadrootsBackgroundTransferIdentifier
  ) throws -> RadrootsBackgroundTransferRequest {
    try RadrootsBackgroundTransferRequest(
      identifier: identifier,
      remoteURL: request.remoteURL,
      method: request.method,
      operation: request.operation,
      headers: request.headers,
      metadata: request.metadata,
      networkPolicy: request.networkPolicy,
      responsePolicy: request.responsePolicy,
      expectedSourceSHA256: request.expectedSourceSHA256,
      maximumTransferBytes: request.maximumTransferBytes
    )
  }

  private static func transferIdentity(
    _ identifier: RadrootsBackgroundTransferIdentifier
  ) -> (draftID: String, revision: UInt64)? {
    let components = identifier.rawValue.split(separator: ".", omittingEmptySubsequences: false)
    guard components.count == 5,
      components[0] == "radroots",
      components[1] == "add",
      components[2].range(of: "^[0-9a-f]{32}$", options: .regularExpression) != nil,
      let revision = UInt64(components[3]),
      String(revision) == components[3],
      components[4].range(of: "^[0-9a-f]{32}$", options: .regularExpression) != nil
    else { return nil }
    return (String(components[2]), revision)
  }

  private static func failure(code: String, message: String) -> RadrootsRuntimeFailure {
    .local(operation: "add.media.background", code: code, safeMessage: message)
  }
}

#if DEBUG
  private actor RadrootsRemoteQualificationMediaPicker: RadrootsMediaPicker {
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
        .inline(RadrootsRemoteQualificationEnvironment.mediaFixtureData()),
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
