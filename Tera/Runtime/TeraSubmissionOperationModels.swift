import Foundation

struct TeraSubmissionMedia: Sendable, Equatable {
  let opaqueReference: String
  let progress: TeraDraftMediaStatus
}

struct TeraSubmissionStatus: Sendable, Equatable, Identifiable, CustomStringConvertible, CustomDebugStringConvertible {
  let request: TeraSubmissionRequest
  let intentID: String
  let operationID: String
  let revision: UInt64
  let captured: TeraComposerDraft
  let state: TeraOutboxState
  let committedAtUnixMilliseconds: UInt64
  let updatedAtUnixMilliseconds: UInt64
  let media: [TeraSubmissionMedia]
  let settlement: TeraOperationSettlement

  var id: String {
    operationID
  }

  var description: String {
    "TeraSubmissionStatus(\(state.rawValue))"
  }

  var debugDescription: String {
    description
  }

  var summary: String {
    if media.contains(where: \.progress.possibleOrphan) {
      return "Photo delivery needs attention. A remote copy may exist."
    }
    switch state {
    case .draft: return "Saved on this device."
    case .mediaPreparing: return "Preparing photo."
    case .mediaUploading: return "Photo upload awaiting verification."
    case .readyToSign, .signing: return "Awaiting signing."
    case .signed: return "Signed; local admission is pending."
    case .queued: return "Queued for the saved relays."
    case .delivering: return "Sending to the saved relays."
    case .partiallyDelivered: return "Partially delivered."
    case .retryable: return "Saved for retry."
    case .terminal: return "Delivery needs attention."
    case .cancelled: return "Local work stopped. Recorded remote effects are retained."
    case .complete: return "Delivery completed for the saved relay policy."
    }
  }

  var mediaSummary: String {
    let verified = media.filter { $0.progress.stage == .verified }.count
    let orphans = media.filter(\.progress.possibleOrphan).count
    let summary = "\(verified) of \(media.count) photos verified"
    return orphans > 0 ? "\(summary); \(orphans) possible orphan" : summary
  }

  /// Translation only: these URLs come from the validated Rust intent.
  var preparedMedia: [TeraPreparedMedia] {
    zip(captured.form.editingValue.media, media).map { source, state in
      TeraPreparedMedia(opaqueReference: source.opaqueReference, remoteURL: state.progress.url,
                        sha256: source.sha256, mediaType: source.mediaType, byteSize: source.byteSize,
                        width: source.width, height: source.height, alt: source.alt,
                        preparedAtUnixSeconds: source.preparedAtUnixSeconds)
    }
  }
}

enum TeraSubmissionSummaryState: Sendable, Equatable {
  case reserved
  case operation(intentID: String, operationID: String, revision: UInt64, state: TeraOutboxState)
}

enum TeraSubmissionRepairReason: Sendable, Equatable {
  case unsupportedSchema
  case corruptRecord
  case needsAttention
}

struct TeraSubmissionSummary: Sendable, Equatable, Identifiable {
  let request: TeraSubmissionRequest
  let reservationID: String
  let reservedAtUnixMilliseconds: UInt64
  let state: TeraSubmissionSummaryState
  var id: String {
    reservationID
  }
}

enum TeraSubmissionEntry: Sendable, Equatable, Identifiable {
  case submission(TeraSubmissionSummary)
  case repair(key: String, revision: UInt64, reason: TeraSubmissionRepairReason)

  var id: String {
    switch self {
    case let .submission(summary): summary.id
    case let .repair(key, _, _): key
    }
  }
}

struct TeraSubmissionPage: Sendable, Equatable {
  let scope: TeraComposerScope
  let entries: [TeraSubmissionEntry]
  let nextCursor: String?
}

struct TeraSubmissionMediaRequest: Sendable, Equatable {
  let request: TeraSubmissionRequest
  let expectedRevision: UInt64
  let media: TeraPreparedMediaHandle
}

struct TeraSubmissionUploadJob: Sendable, Equatable {
  let submission: TeraSubmissionStatus
  let transfer: TeraNativeTransferJob
}
