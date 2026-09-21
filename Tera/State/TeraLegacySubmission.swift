import Foundation

/// Existing legacy and revision behavior; scoped IDs never enter this adapter.
@MainActor
struct TeraLegacySubmission {
  let runtimeClient: TeraRuntimeClient
  let media: (any TeraAddMediaHandling)?
  let revisionID: () -> String?
  let initial: () async throws -> TeraDraftStatus
  let ensure: () throws -> Void
  let acceptDraft: (TeraDraftStatus) throws -> Void
  let acceptRevision: (TeraRevisionStatus) throws -> Void
  let message: (String) -> Void
  let refreshMedia: () async -> Void

  func submit() async throws {
      var status = try await initial()

      try ensure()
      guard status.coordinateWritable else {
        try acceptDraft(status)
        message(status.honestSummary)
        return
      }
      if !status.media.isEmpty,
        status.media.contains(where: { $0.stage != .verified })
      {
        status = try await uploadPendingMedia(status)
      }

      try ensure()
      if status.isRevision {
        let revision = try await runtimeClient.advanceRevision(
          operationID: revisionID() ?? status.id
        )
        try acceptRevision(revision)
        message(revision.honestSummary)
        return
      }

      if status.state.isEditable || status.state == .readyToSign {
        do {
          status = try await runtimeClient.queueAddIntent(
            id: status.id,
            expectedRevision: status.revision
          )
          try acceptDraft(status)
        } catch {
          try ensure()
          if TeraAddPresentation.failure(for: error)?.code == "writable_relay_unavailable" {
            try acceptDraft(status)
            message(status.media.isEmpty
              ? "Draft saved. Configure a writable relay to publish."
              : "Photo verified and draft saved. Configure a writable relay to publish.")
            return
          }
          throw error
        }
      }

      try await advanceSubmittedDraft(status)
  }

  private func advanceSubmittedDraft(
    _ initial: TeraDraftStatus
  ) async throws {
    var status = initial
    do {
      if status.canAdvance {
        status = try await runtimeClient.advanceDraft(
          id: status.id,
          expectedRevision: status.revision
        )
        try acceptDraft(status)
      }
      message(status.honestSummary)
    } catch {
      try ensure()
      // Queueing is the commit point. A later retry must reuse this immutable snapshot.
      message("Saved for retry. \(TeraAddPresentation.message(for: error))")
    }
  }

  private func uploadPendingMedia(_ initial: TeraDraftStatus) async throws -> TeraDraftStatus {
    guard let media else {
      throw TeraRuntimeFailure.local(
        operation: "add.media.upload",
        code: "ios.add.media_unavailable",
        safeMessage: "Prepared photos are unavailable on this device."
      )
    }
    var status = initial
    guard let form = status.form else { return status }
    let opened = try await TeraOpenedMedia.open(form.media, using: media)
    defer { opened.close() }
    try ensure()
    for mediaStatus in status.media where mediaStatus.stage != .verified {
      guard let persisted = form.media.first(where: { $0.remoteURL == mediaStatus.url }),
        let handle = opened.handles.first(where: {
          $0.media.opaqueReference == persisted.opaqueReference
        })
      else {
        throw TeraRuntimeFailure.local(
          operation: "add.media.upload",
          code: "ios.add.media_missing",
          safeMessage: "A prepared photo is unavailable."
        )
      }
      let intent = TeraBlossomUploadIntent(
        draftID: status.id,
        expectedRevision: status.revision,
        media: handle
      )
      if try await media.prefersSharedForegroundUpload(ownerID: status.id) {
        status = try await uploadForeground(intent, progress: mediaStatus)
        try acceptDraft(status)
        await refreshMedia()
        try ensure()
        continue
      }
      let job = try await runtimeClient.prepareAddMediaBackground(input: intent)
      try acceptDraft(job.draft)
      let receipt = try await media.uploadInBackground(job: job, media: persisted)
      try ensure()
      status = try await TeraAddUploadCompletion.complete(receipt, handle: handle, media: media, runtimeClient: runtimeClient)
      try await media.settleBackgroundUpload(identifier: receipt.identifier, accepted: true)
      _ = await TeraNativeRecoveryClassification.report(receipt.identifier, reason: .resolved, client: runtimeClient)
      try acceptDraft(status)
      await refreshMedia()
      try ensure()
    }
    return status
  }

  private func uploadForeground(_ intent: TeraBlossomUploadIntent, progress: TeraDraftMediaStatus) async throws -> TeraDraftStatus {
    try ensure()
    guard progress.stage != .uploading, !progress.possibleOrphan else {
      throw TeraRuntimeFailure.local(operation: "add.media.upload", code: "ios.add.media_recovery_required",
                                     safeMessage: "The previous photo upload must be checked before trying again.")
    }
    return try await Task { try await runtimeClient.uploadAddMediaIntent(input: intent) }.value
  }
}
