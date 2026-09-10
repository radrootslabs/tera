import SwiftUI

struct TeraCalendarMetadata: View {
  let card: TeraTodayCard
  let presentation: TeraTodayCardPresentation
  @Environment(\.locale) private var locale
  @Environment(\.timeZone) private var timeZone

  var body: some View {
    VStack(alignment: .leading, spacing: 4) {
      if let timing = card.calendarTiming {
        Label(TeraCalendarPresentation(locale: locale, timeZone: timeZone).summary(timing), systemImage: "calendar")
      }
      if let location = card.location {
        Label(presentation.label(location), systemImage: "mappin.and.ellipse")
      }
    }
    .font(.subheadline)
  }
}
