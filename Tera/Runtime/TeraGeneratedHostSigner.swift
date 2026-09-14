import Foundation
import TeraKitBindings

final class TeraGeneratedHostSigner: TeraHostSigner, @unchecked Sendable {
  typealias QualificationEvidenceRecorder = @Sendable (HostSigningRequest, String) throws -> Void

  private let signer: any TeraRuntimeSigner
  private let clock: TeraClock
  private let qualificationEvidenceRecorder: QualificationEvidenceRecorder

  init(
    signer: any TeraRuntimeSigner,
    clock: TeraClock = .system,
    qualificationEvidenceRecorder: @escaping QualificationEvidenceRecorder =
      TeraRemoteQualificationEvidence.recordBlossomAuthorization
  ) {
    self.signer = signer
    self.clock = clock
    self.qualificationEvidenceRecorder = qualificationEvidenceRecorder
  }

  func signerStatus() async -> SignerStatusRecord {
    await SignerStatusRecord(
      schemaVersion: 1,
      availability: signer.availability().generatedValue
    )
  }

  func sign(request: HostSigningRequest) async -> HostSigningResult {
    guard (try? clock.unixMilliseconds()) != nil else {
      return failedSigningResult(for: request)
    }
    let purpose = request.purpose.appValue
    let outcome = await signer.sign(
      TeraRuntimeSigningRequest(
        operationID: request.operationId,
        signerRequestID: request.signerRequestId,
        publicKeyHex: request.publicKey,
        purpose: purpose,
        deadlineUnixMilliseconds: request.deadlineUnixMs,
        digest: request.eventIdDigest
      )
    )
    #if DEBUG
      if request.purpose == .blossomUpload,
         let signatureHex = outcome.signatureHex
      {
        do {
          try qualificationEvidenceRecorder(request, signatureHex)
        } catch {
          return failedSigningResult(for: request)
        }
      }
    #endif
    let completedAtUnixMilliseconds = try? clock.unixMilliseconds()
    guard completedAtUnixMilliseconds != nil || (request.purpose == .nostrEvent && outcome.signatureHex != nil) else {
      return failedSigningResult(for: request)
    }
    return HostSigningResult(
      schemaVersion: 1,
      outcome: outcome.generatedOutcome,
      operationId: request.operationId,
      signerRequestId: request.signerRequestId,
      publicKey: request.publicKey,
      purpose: request.purpose,
      signatureHex: outcome.signatureHex,
      completedAtUnixMs: completedAtUnixMilliseconds ?? 0
    )
  }

  private func failedSigningResult(for request: HostSigningRequest) -> HostSigningResult {
    HostSigningResult(
      schemaVersion: 1,
      outcome: .failed,
      operationId: request.operationId,
      signerRequestId: request.signerRequestId,
      publicKey: request.publicKey,
      purpose: request.purpose,
      signatureHex: nil,
      completedAtUnixMs: 0
    )
  }
}

extension TeraRuntimeSignerAvailability {
  fileprivate var generatedValue: SignerAvailabilityRecord {
    switch self {
    case .ready: .ready
    case .busy: .busy
    case .locked: .locked
    case .unavailable: .unavailable
    }
  }
}

extension HostSigningPurpose {
  fileprivate var appValue: TeraRuntimeSigningPurpose {
    switch self {
    case .nostrEvent: .nostrEvent
    case .blossomUpload: .blossomUpload
    }
  }
}

extension TeraRuntimeSigningOutcome {
  fileprivate var generatedOutcome: HostSigningOutcome {
    switch self {
    case .signed: .signed
    case .locked: .locked
    case .cancelled: .cancelled
    case .rejected: .rejected
    case .timedOut: .timedOut
    case .unavailable: .unavailable
    case .invalidated: .invalidated
    case .failed: .failed
    }
  }

  fileprivate var signatureHex: String? {
    if case let .signed(signatureHex) = self {
      return signatureHex
    }
    return nil
  }
}
