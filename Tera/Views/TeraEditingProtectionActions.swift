import SwiftUI

/// Present a discard confirmation only from the surface the user is acting on,
/// including when the editor is underneath the saved-work sheet.
struct TeraEditingProtectionActions: View {
  @ObservedObject var protection: TeraEditingProtection
  @State private var confirmsDiscard = false
  @State private var didConfirmDiscard = false
  @State private var discardToken: UUID?

  var body: some View {
    Group {
      if protection.failed {
        failureSection
      }
    }
    .confirmationDialog("Discard the current unsaved changes?", isPresented: $confirmsDiscard, titleVisibility: .visible) {
      Button("Discard and continue", role: .destructive) {
        didConfirmDiscard = true
        if let discardToken {
          protection.discard(token: discardToken)
        }
      }
      Button("Keep editing", role: .cancel) { cancelConfirmation() }
    } message: {
      Text("Previously saved drafts and submitted operations remain on this device.")
    }
    .onChange(of: confirmsDiscard) { _, presented in
      // Popovers omit the cancel button. A late dismissal belongs only to the
      // choice that opened it, even if another request has already failed.
      if !presented, !didConfirmDiscard {
        cancelConfirmation()
      }
    }
  }

  private var failureSection: some View {
    Section("Keep your editing") {
      Text("Saving could not be confirmed. Your current editing is still here.")
      Button("Retry save and continue") { protection.retry() }
        .accessibilityIdentifier("tera.add.protection.retry")
      Button("Discard unsaved changes…", role: .destructive) {
        discardToken = protection.choiceToken
        didConfirmDiscard = false
        confirmsDiscard = true
      }
      .accessibilityIdentifier("tera.add.protection.discard")
      Button("Keep editing") { protection.cancel() }
        .accessibilityIdentifier("tera.add.protection.keep")
    }
  }

  private func cancelConfirmation() {
    if let discardToken {
      protection.cancel(token: discardToken)
    }
  }
}
