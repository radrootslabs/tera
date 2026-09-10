import Foundation

extension TeraRuntimeClient {
  func reconcileToday(request: TeraTodayReconcileRequest) async throws -> TeraTodayPage {
    do {
      return try await runtimeOperation("runtime.today.reconcile") { backend in
        try await backend.reconcileToday(request: request)
      }
    } catch let error as TeraRuntimeClientError {
      throw error
    } catch {
      throw TeraRuntimeClientError.today(Self.failure(from: error, operation: "runtime.today.reconcile"))
    }
  }

  func todayPage(request: TeraTodayPageRequest) async throws -> TeraTodayPage {
    do {
      return try await runtimeOperation("runtime.today.page") { backend in
        try await backend.todayPage(request: request)
      }
    } catch let error as TeraRuntimeClientError {
      throw error
    } catch {
      throw TeraRuntimeClientError.today(
        Self.failure(from: error, operation: "runtime.today.page")
      )
    }
  }

  func refreshToday(
    context: TeraLocalNetwork,
    nowUnixSeconds: UInt64,
    update: TeraTodayProjectionUpdate = .incremental,
    backfillCursor: String? = nil
  ) async throws -> TeraTodaySyncReceipt {
    do {
      return try await runtimeOperation("runtime.today.refresh") { backend in
        try await backend.refreshToday(
          context: context,
          nowUnixSeconds: nowUnixSeconds,
          update: update, backfillCursor: backfillCursor
        )
      }
    } catch let error as TeraRuntimeClientError {
      throw error
    } catch {
      throw TeraRuntimeClientError.today(
        Self.failure(from: error, operation: "runtime.today.refresh")
      )
    }
  }
}
