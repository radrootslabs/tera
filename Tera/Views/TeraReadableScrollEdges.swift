import SwiftUI

extension View {
  @ViewBuilder
  func teraReadableScrollEdges(_ enabled: Bool) -> some View {
    // At accessibility sizes, the iOS 26 edge fade can obscure several lines
    // of otherwise visible instructions. Keep text opaque up to the bars.
    if #available(iOS 26.0, *) {
      scrollEdgeEffectHidden(enabled, for: .all)
    } else {
      self
    }
  }
}
