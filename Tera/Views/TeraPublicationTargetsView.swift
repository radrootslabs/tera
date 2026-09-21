import SwiftUI

struct TeraPublicationTargetsView: View {
  let details: TeraPublicationTargets

  var body: some View {
    DisclosureGroup("Saved relay evidence") {
      Text(details.policySummary).font(.footnote)
      ForEach(details.targets) { target in
        VStack(alignment: .leading, spacing: 4) {
          Text(target.endpoint).font(.caption).textSelection(.enabled)
          if details.isRequired(target.id) {
            Text("Required by the saved policy").font(.caption)
          }
          Text(target.summary)
          if target.accepted, target.rejected {
            Text("A separate refusal is also recorded.").font(.caption)
          }
          if let observed = target.readBackObservedAtUnixMilliseconds {
            Text("Read-back observed on \(Date(timeIntervalSince1970: Double(observed) / 1000).formatted()).")
              .font(.caption)
          } else {
            Text("No read-back observation is shown for this relay.").font(.caption)
          }
        }
      }
      if !details.readBackAvailable {
        Text("Saved read-back evidence is unavailable.").font(.footnote)
      } else if !details.readBackComplete {
        Text("Read-back evidence shown is a bounded subset.").font(.footnote)
      }
      Text("Read-back is separate from acceptance. Neither proves continued retention or delivery to every relay.")
        .font(.footnote)
    }
    .accessibilityIdentifier("tera.submission.relay_evidence")
  }
}
