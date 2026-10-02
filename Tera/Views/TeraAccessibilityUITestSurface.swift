#if DEBUG
  import RadrootsKit
  import SwiftUI

  /// Isolated presentation qualification: real stores and SQLite, no credentials
  /// or external relays. The photo fixture cannot confirm durability or upload bytes.
  struct TeraAccessibilityUITestSurface: View {
    @StateObject private var fixture = TeraAccessibilityFixture()
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.dynamicTypeSize) private var textSize
    private let scenario = ProcessInfo.processInfo.environment["TERA_IOS_UI_TEST_ACCESSIBILITY"]

    var body: some View {
      Group {
        if scenario == "failure" || scenario == "identity" {
          RuntimeStatusView(phase: scenario == "identity" ? .identityRequired : .failed(.local(
            operation: "fixture", code: "ios.fixture.unavailable", safeMessage: "Local state is unavailable."
          )), retry: { fixture.retried = true }, createIdentity: {}, importIdentity: { _ in },
                            unlockIdentity: {}, recoverIdentity: {}, applyConfigurationReconfiguration: {})
          .overlay(alignment: .bottom) {
            if fixture.retried {
              Text("Local state checked").accessibilityIdentifier("tera.test.accessibility.retried")
            }
          }
        } else if let stores = fixture.stores, let snapshot = fixture.snapshot {
          TeraRootShell(snapshot: snapshot, stores: stores)
        } else {
          Text(fixture.failure ?? "Preparing isolated accessibility fixture")
        }
      }
      .accessibilityElement(children: .contain)
      .accessibilityIdentifier("tera.test.accessibility.root")
      .accessibilityValue(Text(verbatim: "text=\(textSize); reduceMotion=\(reduceMotion)"))
      .task { await fixture.start() }
      .overlay {
        if scenario == "save-error", !fixture.hasPreparedSaveError {
          Button("Prepare unconfirmed Save") {
            Task { await fixture.prepareSaveError() }
          }
          .buttonStyle(.borderedProminent)
        }
      }
    }
  }

  @MainActor
  private final class TeraAccessibilityFixture: ObservableObject {
    @Published var stores: TeraProductStores?
    @Published var snapshot: TeraRuntimeSnapshot?
    @Published var failure: String?
    @Published var retried = false
    @Published var hasPreparedSaveError = false
    private let client = TeraRuntimeClient.production()

    func start() async {
      guard stores == nil, failure == nil else { return }
      do {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("tera-accessibility-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let publicKey = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
        let roots = try RadrootsAppleFileRoots(appIdentifier: "test.tera.accessibility", dataRoot: root,
                                               cacheRoot: root, temporaryRoot: root)
        let mobileStore = try RadrootsAppleMobileStore.prepare(roots: roots, publicKeyHex: publicKey,
                                                               protectedDataAvailability: .available)
        let configuration = TeraRuntimeLaunchConfiguration(
          applicationSupportDirectory: mobileStore.applicationSupportDirectory.path,
          publicKeyHex: publicKey,
          sourceGenerationHex: String(repeating: "04", count: 32), sourceGenerationCreatedAtUnixMilliseconds: 1_800_000_000_000,
          protectedData: .available, networkProfile: .simulator, writableRelays: ["ws://127.0.0.1:19999"], blossom: nil,
          app: .init(bundleIdentifier: "test.tera.accessibility", version: "1", buildNumber: "1", buildSHA: nil),
          signerGeneration: "accessibility-fixture", signer: TeraAccessibilitySigner(), adoptBootstrapSettings: false
        )
        let snapshot = try await client.start(configuration: configuration)
        let stores = TeraProductStores(runtimeClient: client, addMedia: TeraAccessibilityMedia())
        stores.configure(snapshot: snapshot)
        await stores.resume()
        if let label = ProcessInfo.processInfo.environment["TERA_IOS_UI_TEST_FORM"],
          let type = TeraAddCommandType.allCases.first(where: { $0.label == label })
        {
          stores.add.selectType(type)
        }
        self.snapshot = snapshot
        self.stores = stores
      } catch {
        if case let TeraRuntimeClientError.startup(report) = error {
          failure = "Accessibility fixture failed to start: " + report.code
        } else {
          failure = "Accessibility fixture failed to start."
        }
      }
    }

    func prepareSaveError() async {
      guard let stores, !hasPreparedSaveError else { return }
      await stores.add.importPhotos()
      await stores.add.save()
      hasPreparedSaveError = true
    }
  }

  private struct TeraAccessibilitySigner: TeraRuntimeSigner {
    func availability() async -> TeraRuntimeSignerAvailability {
      .unavailable
    }

    func sign(_: TeraRuntimeSigningRequest) async -> TeraRuntimeSigningOutcome {
      .unavailable
    }
  }

  private struct TeraAccessibilityMedia: TeraAddMediaHandling {
    func support() async throws -> TeraAddMediaSupport {
      .init(library: true, camera: false)
    }

    func importImages(limit: Int) async throws -> [TeraPreparedMedia] {
      guard limit > 0 else { return [] }
      return [.init(opaqueReference: "media:" + String(repeating: "0", count: 64), remoteURL: nil,
                    sha256: String(repeating: "0", count: 64), mediaType: "image/png", byteSize: 4,
                    width: 2, height: 2, alt: "", preparedAtUnixSeconds: 1_800_000_000)]
    }

    func captureImage() async throws -> TeraPreparedMedia {
      throw TeraComposerAcknowledgment.unconfirmed
    }

    func open(_: [TeraPreparedMedia]) async throws -> TeraOpenedMedia {
      throw TeraComposerAcknowledgment.unconfirmed
    }
  }
#endif
