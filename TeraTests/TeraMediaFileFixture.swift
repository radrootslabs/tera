import Foundation
@testable import TeraApp

enum TeraMediaFileFixture {
  static func open(_ media: [TeraPreparedMedia], bytes: Data) throws -> TeraOpenedMedia {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try bytes.write(to: url)
    defer { try? FileManager.default.removeItem(at: url) }
    let original = try FileHandle(forReadingFrom: url)
    defer { try? original.close() }
    return try TeraOpenedMedia(
      handles: media.map {
        try TeraPreparedMediaHandle(media: $0, fileDescriptor: UInt64(original.fileDescriptor))
      },
      files: []
    )
  }
}
