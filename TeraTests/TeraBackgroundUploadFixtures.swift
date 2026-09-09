import CryptoKit
import RadrootsKit
@testable import TeraApp
import XCTest

final class BackgroundUploadFixture: @unchecked Sendable {
  let draftID: String
  let media: TeraPreparedMedia
  private let root: URL
  private let roots: RadrootsAppleFileRoots

  init(draftID: String = String(repeating: "1", count: 32)) throws {
    self.draftID = draftID
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
    media = TeraPreparedMedia(
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

  func coordinator(transfer: any RadrootsBackgroundTransfer) -> TeraAddMediaCoordinator {
    TeraAddMediaCoordinator(
      roots: roots,
      picker: BackgroundMediaPicker(),
      preparer: RadrootsAppleMediaPreparer(roots: roots),
      transfer: transfer
    )
  }

  func job(revision: UInt64, operation: String) -> TeraNativeUploadJob {
    TeraNativeUploadJob(
      operationID: operation,
      draft: draft(revision: revision, stage: .uploading),
      remoteURL: media.remoteURL!,
      authorizationHeader: "Nostr test-authorization",
      expectedSHA256: media.sha256,
      mediaType: media.mediaType,
      byteSize: media.byteSize
    )
  }

  func draft(revision: UInt64, stage: TeraDraftMediaStage) -> TeraDraftStatus {
    var form = TeraAddForm.empty(.createPhotoUpdate)
    form.content = "Background transfer"
    form.media = [media]
    return TeraDraftStatus(
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
        TeraDraftMediaStatus(
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
    job: TeraNativeUploadJob,
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

extension TeraNativeUploadJob {
  var transferIdentifier: String {
    "radroots.add.\(draft.id).\(draft.revision).\(operationID)"
  }
}

struct BackgroundMediaPicker: RadrootsMediaPicker {
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

actor BackgroundTransferHarness: RadrootsBackgroundTransfer {
  private var discoveryPause: ResourceTestPause?
  private var retryPause: ResourceTestPause?
  private var discoveryCallback: (@Sendable () async -> Void)?
  private(set) var discoveryCount = 0
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

  func pauseDiscovery(_ pause: ResourceTestPause) {
    discoveryPause = pause
  }

  func pauseRetry(_ pause: ResourceTestPause) {
    retryPause = pause
  }

  func onDiscovery(_ callback: @escaping @Sendable () async -> Void) {
    discoveryCallback = callback
  }

  var counts: BackgroundTransferCounts {
    BackgroundTransferCounts(
      enqueue: enqueueCount, retry: retryCount, cancel: cancelCount,
      acceptedSettlement: acceptedSettlementCount
    )
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
    let admissionPause = retryPause
    retryPause = nil
    await admissionPause?.wait()
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
    discoveryCount += 1
    let admissionPause = discoveryPause
    discoveryPause = nil
    await admissionPause?.wait()
    let callback = discoveryCallback
    discoveryCallback = nil
    await callback?()
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

enum BackgroundTransferPause: Sendable {
  case none
  case discovery
  case snapshot
}

struct BackgroundTransferCounts: Sendable {
  let enqueue: Int
  let retry: Int
  let cancel: Int
  let acceptedSettlement: Int
}
