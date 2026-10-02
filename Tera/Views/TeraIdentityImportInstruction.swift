import SwiftUI
import UIKit

/// An intrinsically sized, wrapping instruction in the secure import form.
struct TeraIdentityImportInstruction: UIViewRepresentable {
  let text: String

  func makeUIView(context _: Context) -> UILabel {
    let label = UILabel()
    label.font = .preferredFont(forTextStyle: .body)
    label.adjustsFontForContentSizeCategory = true
    label.textColor = .label
    label.numberOfLines = 0
    label.lineBreakMode = .byWordWrapping
    label.setContentCompressionResistancePriority(.required, for: .vertical)
    label.isAccessibilityElement = true
    return label
  }

  func updateUIView(_ label: UILabel, context _: Context) {
    label.text = text
  }

  func sizeThatFits(_ proposal: ProposedViewSize, uiView: UILabel, context _: Context) -> CGSize? {
    guard let width = proposal.width, width.isFinite, width > 0 else { return nil }
    let size = uiView.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
    return CGSize(width: width, height: ceil(size.height))
  }
}
