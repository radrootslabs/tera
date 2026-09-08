import SwiftUI

public enum TeraAppRelease: Sendable {
  public static let version = "0.1.0-alpha"
}

public struct TeraAppView: View {
  public init() {}

  public var body: some View {
    TeraProvider {
      AppEntry()
    }
  }
}

struct AppEntry: View {
  @EnvironmentObject private var appModel: TeraAppModel

  var body: some View {
    Group {
      if case let .running(snapshot) = appModel.phase {
        TeraRootShell(
          snapshot: snapshot,
          stores: appModel.productStores
        )
      } else {
        RuntimeStatusView(
          phase: appModel.phase,
          retry: { Task { await appModel.retry() } },
          createIdentity: { Task { await appModel.createIdentity() } },
          importIdentity: { material in
            Task { await appModel.importIdentity(material) }
          },
          unlockIdentity: { Task { await appModel.unlockIdentity() } },
          recoverIdentity: { Task { await appModel.recoverIdentity() } },
          applyConfigurationReconfiguration: {
            Task { await appModel.applyConfigurationReconfiguration() }
          }
        )
      }
    }
    .accessibilityIdentifier("radroots.app_entry")
  }
}
