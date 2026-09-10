import SwiftUI

struct TeraTodayStatusView: View {
  let presentation: TeraTodayPresentation

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      if presentation.refresh == .refreshing {
        ProgressView("Checking for updates…")
      }
      if presentation.isReading {
        ProgressView("Reading saved posts…")
      }
      if case let .failed(failure) = presentation.refresh {
        Label("Refresh failed. \(failure.message)", systemImage: failure.systemImage)
      }
      ForEach(Array(presentation.relayMessages.enumerated()), id: \.offset) { _, message in
        Text(message)
      }
      if let failure = presentation.readFailure {
        Label(failure.readStatus, systemImage: failure.systemImage)
      }
      if let message = presentation.freshnessMessage {
        Text(message)
      }
    }
    .font(.footnote)
    .foregroundStyle(.secondary)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel("Today status")
    .accessibilityValue(presentation.accessibilityStatus)
    .accessibilityIdentifier("radroots.today.status")
  }
}
