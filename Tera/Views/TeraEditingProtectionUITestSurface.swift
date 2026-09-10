#if DEBUG
  import SwiftUI

  /// The existing runtime-free shell test mode hosts the actual failure controls.
  /// Production persistence and relaunch are qualified by the native FFI tests.
  struct TeraEditingProtectionUITestSurface: View {
    @StateObject private var protection = TeraEditingProtection()
    @State private var editing = "Keep this unfinished text"
    @State private var replacements = 0

    var body: some View {
      Form {
        TextField("Editing", text: $editing)
          .accessibilityIdentifier("tera.test.editing.value")
        TeraEditingProtectionActions(protection: protection)
        Button("Attempt replacement") {
          protection.schedule(kind: .editing, save: { false }, apply: {
            editing = ""
            replacements += 1
            return true
          })
        }
        .accessibilityIdentifier("tera.test.editing.replace")
        Text(String(replacements)).accessibilityIdentifier("tera.test.editing.replacements")
      }
    }
  }
#endif
