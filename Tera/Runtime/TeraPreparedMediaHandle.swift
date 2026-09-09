import TeraKitBindings

/// Admission is synchronous while the coordinator still owns the original file.
/// Async closures retain this immutable Rust owner even after native close.
struct TeraPreparedMediaHandle: Sendable, Equatable {
  let media: TeraPreparedMedia
  private let file: FfiMediaFile

  init(media: TeraPreparedMedia, fileDescriptor: UInt64) throws {
    self.media = media
    file = try FfiMediaFile(fileDescriptor: fileDescriptor, byteSize: media.byteSize)
  }

  static func == (lhs: Self, rhs: Self) -> Bool {
    // This is transient owner identity, never a persistent media/request key.
    lhs.media == rhs.media && lhs.file === rhs.file
  }

  var generatedValue: FfiPreparedMediaInput {
    FfiPreparedMediaInput(
      schemaVersion: 2,
      opaqueReference: media.opaqueReference,
      file: file,
      sha256: media.sha256,
      mediaType: media.mediaType,
      byteSize: media.byteSize,
      width: media.width,
      height: media.height,
      alt: media.alt,
      preparedAtUnixS: media.preparedAtUnixSeconds
    )
  }
}
