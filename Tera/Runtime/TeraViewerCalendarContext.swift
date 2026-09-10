import Foundation
import TeraKitBindings

struct TeraViewerCalendarContext: Sendable, Equatable {
  let asOfUnixSeconds: UInt64
  let timeZoneID: String
  let timeZone: TimeZone
  let civilDate: TeraCivilDate
}

extension FfiViewerCalendarContext {
  func appValue() throws -> TeraViewerCalendarContext {
    guard schemaVersion == 1, asOfUnixS > 0,
      TeraCalendarTiming.presentationInstant(asOfUnixS) != nil,
      timeZone.utf8.count <= 255, let zone = TimeZone(identifier: timeZone)
    else { throw TeraCalendarTiming.unsupported }
    // Rust's pinned database owns the relevance date. Do not recalculate it
    // with potentially different OS timezone rules at the native boundary.
    return try TeraViewerCalendarContext(
      asOfUnixSeconds: asOfUnixS, timeZoneID: timeZone,
      timeZone: zone, civilDate: civilDate.appValue()
    )
  }
}
