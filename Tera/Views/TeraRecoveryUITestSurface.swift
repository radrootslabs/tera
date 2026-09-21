#if DEBUG
  import SwiftUI

  /// Runtime-free shell fixture using the same fact presentation and controls
  /// as Add and Drafts. It has no transport, credentials or durable authority.
  struct TeraRecoveryUITestSurface: View {
    @State private var editing = "Keep editing during recovery"
    @State private var scenario = 0
    @State private var checked = "none"
    private let key = String(repeating: "a", count: 64)

    var body: some View {
      Form {
        TextField("Editing", text: $editing).accessibilityIdentifier("tera.test.recovery.editing")
        Button("Next recovery state") { scenario = (scenario + 1) % 4 }
          .accessibilityIdentifier("tera.test.recovery.next")
        Text(checked).accessibilityIdentifier("tera.test.recovery.checked")
        TeraNativeRepairProgressView(message: pause?.message, remaining: 12)
        TeraNativeRepairActions(issue: .init(key: key, reason: scenario == 3 ? .associationMismatch : .outcomeUnconfirmed, status: nil),
                                index: 0, isRunning: false) { checked = key }
      }
    }

    private var pause: TeraNativeRecoveryPause? {
      switch scenario {
      case 1: .protectedData
      case 2: .quota
      default: nil
      }
    }
  }
#endif
