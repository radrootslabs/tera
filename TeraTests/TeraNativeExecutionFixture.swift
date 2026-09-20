import Foundation
import RadrootsKit
@testable import TeraApp

actor NativeExecutionTransfer: RadrootsBackgroundTransfer {
  let base: BackgroundTransferHarness
  let unknownInventory: Bool
  let replaceExecution: Bool
  private var reads = 0

  init(base: BackgroundTransferHarness = BackgroundTransferHarness(), unknownInventory: Bool = false,
       replaceExecution: Bool = false)
  {
    self.base = base
    self.unknownInventory = unknownInventory
    self.replaceExecution = replaceExecution
  }

  func enqueue(_ request: RadrootsBackgroundTransferRequest) async throws -> RadrootsBackgroundTransferHandle {
    _ = try await base.enqueue(request)
    throw RadrootsBackgroundTransferError.transferFailure
  }

  func retry(_ request: RadrootsBackgroundTransferRequest) async throws -> RadrootsBackgroundTransferHandle {
    _ = try await base.retry(request)
    throw RadrootsBackgroundTransferError.transferFailure
  }

  func snapshot(for identifier: RadrootsBackgroundTransferIdentifier) async throws -> RadrootsBackgroundTransferSnapshot? {
    if unknownInventory {
      throw RadrootsBackgroundTransferError.transferFailure
    }
    reads += 1
    let value = try await base.snapshot(for: identifier)
    guard replaceExecution, reads > 1, let value else { return value }
    return try RadrootsBackgroundTransferSnapshot(request: value.request, state: value.state, executionID: UUID())
  }

  func snapshots() async throws -> [RadrootsBackgroundTransferSnapshot] {
    if unknownInventory {
      throw RadrootsBackgroundTransferError.transferFailure
    }
    return try await base.snapshots()
  }

  func cancel(_ identifier: RadrootsBackgroundTransferIdentifier) async throws {
    try await base.cancel(identifier)
  }

  func expire(_ identifier: RadrootsBackgroundTransferIdentifier) async throws {
    try await base.expire(identifier)
  }

  func settle(_ identifier: RadrootsBackgroundTransferIdentifier, verification: RadrootsBackgroundTransferVerification) async throws {
    try await base.settle(identifier, verification: verification)
  }

  func handleEventsForBackgroundURLSession(identifier _: String, completionHandler: @escaping @Sendable () -> Void) async {
    completionHandler()
  }
}

actor LostNativeAdmission {
  private(set) var starts = 0
  private var active: Set<RadrootsBackgroundTransferIdentifier> = []

  nonisolated var adapters: RadrootsAppleBackgroundTransferAdapters {
    .init(enqueue: { request, _ in try await self.enqueue(request) },
          cancel: { _ in throw RadrootsBackgroundTransferError.transferFailure },
          activeTransferIdentifiers: { await self.identifiers() },
          handleBackgroundEvents: { _, completion in completion() })
  }

  func enqueue(_ request: RadrootsBackgroundTransferRequest) throws {
    starts += 1
    active.insert(request.identifier)
    throw RadrootsBackgroundTransferError.transferFailure
  }

  func identifiers() -> Set<RadrootsBackgroundTransferIdentifier> {
    active
  }
}
