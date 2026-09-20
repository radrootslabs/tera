import Foundation
import RadrootsKit

@MainActor
final class TeraDiagnosticsStore: ObservableObject {
  @Published var preparedExport: RadrootsPreparedExportDocument?
  @Published private(set) var isPreparing = false
  @Published private(set) var message: String?

  private let coordinator: TeraLifecycleCoordinator
  private var activeExport: RadrootsPreparedExportDocument?

  init(coordinator: TeraLifecycleCoordinator) {
    self.coordinator = coordinator
  }

  func prepare(snapshot: TeraRuntimeSnapshot, bundle: Bundle = .main) async {
    guard !isPreparing, preparedExport == nil, activeExport == nil else { return }
    isPreparing = true
    message = nil
    defer { isPreparing = false }
    do {
      let export = try await coordinator.prepareDiagnostics(
        snapshot: snapshot,
        appVersion: bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
          ?? "0",
        appBuild: bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "0",
        phase: "running"
      )
      activeExport = export
      preparedExport = export
    } catch {
      activeExport = nil
      preparedExport = nil
      message = TeraUserMessages.text(.diagnosticsPrepareFailed)
    }
  }

  func completeExport(_ result: Result<RadrootsExportDocumentResult, Error>) {
    let export = activeExport
    activeExport = nil
    preparedExport = nil
    switch result {
    case .success:
      message = TeraUserMessages.text(.diagnosticsExportSucceeded)
    case .failure:
      message = TeraUserMessages.text(.diagnosticsExportFailed)
    }
    guard let export else { return }
    Task { await coordinator.releaseDiagnostics(export) }
  }
}
