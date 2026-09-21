import Foundation

enum TeraOutboxState: String, CaseIterable, Sendable, Equatable, Hashable {
  case draft
  case mediaPreparing
  case mediaUploading
  case readyToSign
  case signing
  case signed
  case queued
  case delivering
  case partiallyDelivered
  case retryable
  case terminal
  case cancelled
  case complete

  var isEditable: Bool {
    self == .draft || self == .mediaPreparing
  }

  var canAdvance: Bool {
    self == .queued || self == .retryable || self == .partiallyDelivered
  }

  var canCancel: Bool {
    ![.cancelled, .complete, .terminal].contains(self)
  }

  var label: String {
    switch self {
    case .draft: "Saved on this device."
    case .mediaPreparing: "Preparing photo."
    case .mediaUploading: "Photo upload awaiting verification."
    case .readyToSign, .signing: "Awaiting signing."
    case .signed: "Signed; local admission is pending."
    case .queued: "Queued for the saved relays."
    case .delivering: "Sending to the saved relays."
    case .partiallyDelivered: "Partially delivered; review the saved relay outcomes."
    case .retryable: "Saved for retry."
    case .terminal: "Publication needs attention."
    case .cancelled: "Local work stopped. Recorded remote effects are retained."
    case .complete: "Delivery completed for the saved relay policy."
    }
  }

  func summary(settlement: TeraOperationSettlement?) -> String {
    guard let settlement, !settlement.summary.isEmpty else { return label }
    return "\(label) \(settlement.summary)"
  }
}

extension TeraOperationSettlement {
  /// Counts are retained facts, not a global publication or erasure receipt.
  /// A satisfied plan must never hide other pending or uncertain work.
  var summary: String {
    var facts: [String] = []
    if deliverySatisfied > 0 {
      facts.append("Saved relay requirements met for \(deliverySatisfied) of \(deliveryPlans) delivery plans.")
    }
    if indeterminate > 0 {
      facts.append("\(indeterminate) artifact outcomes remain unknown.")
    }
    if deliveryPending > 0 || pending > 0 {
      facts.append("Work remains pending.")
    }
    if deliveryRetryable > 0 || retryable > 0 {
      facts.append("Work is saved for retry; current permissions must be checked.")
    }
    if deliveryExhausted > 0 {
      facts.append("A saved delivery limit was reached; review is needed.")
    }
    if failedTerminal > 0 || deliveryFailedTerminal > 0 {
      facts.append("Some work needs attention after failure.")
    }
    if cancelled > 0 || deliveryCancelled > 0 {
      facts.append("Some local work was stopped; recorded effects are retained.")
    }
    if signed > 0 {
      facts.append("\(signed) signed artifacts retained; \(admitted) admitted locally.")
    }
    return facts.joined(separator: " ")
  }
}
