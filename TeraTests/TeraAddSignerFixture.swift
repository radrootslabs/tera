import Foundation
@testable import TeraApp

struct AddSigner: TeraRuntimeSigner {
  func availability() async -> TeraRuntimeSignerAvailability {
    .ready
  }

  func sign(_: TeraRuntimeSigningRequest) async -> TeraRuntimeSigningOutcome {
    .failed
  }
}
