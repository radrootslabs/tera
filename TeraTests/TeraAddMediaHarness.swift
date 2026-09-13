import Foundation
@testable import TeraApp

actor AddMediaHarness: TeraAddMediaHandling {
  private let delayFirstUpload: Bool
  private let delaySettlement: Bool
  private let openPause: ResourceTestPause?
  private let failOpen: Bool
  private var uploadAttempts = 0
  private var settlementStarted = false
  private var settlements: [Bool] = []
  private var reconciliations = 0
  private(set) var openCount = 0
  private(set) var submissionReconciliations = 0
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

  init(delayFirstUpload: Bool = false, delaySettlement: Bool = false,
       openPause: ResourceTestPause? = nil, failOpen: Bool = false)
  {
    self.delayFirstUpload = delayFirstUpload
    self.delaySettlement = delaySettlement
    self.openPause = openPause
    self.failOpen = failOpen
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

  func open(_ media: [TeraPreparedMedia]) async throws -> TeraOpenedMedia {
    openCount += 1
    await openPause?.wait()
    if failOpen {
      throw TeraComposerAcknowledgment.unconfirmed
    }
    return try TeraMediaFileFixture.open(media, bytes: Data(repeating: 0, count: 4))
  }

  func uploadInBackground(
    transfer job: TeraNativeTransferJob,
    media _: TeraPreparedMedia
  ) async throws -> TeraAddBackgroundUploadReceipt {
    uploadAttempts += 1
    if delayFirstUpload, uploadAttempts == 1 {
      try await Task.sleep(nanoseconds: 50_000_000)
    }
    return TeraAddBackgroundUploadReceipt(
      identifier: "radroots.add.\(job.ownerID).\(job.expectedRevision).\(job.operationID)",
      draftID: job.ownerID,
      expectedRevision: job.expectedRevision,
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

  func reconcileBackgroundSubmissions(_: [TeraSubmissionStatus]) {
    submissionReconciliations += 1
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
