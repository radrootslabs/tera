import Foundation
import Synchronization

/// Owns the native originals until exactly one caller takes them for close.
/// Immutable Rust handles retain their independent files through late FFI work.
final class TeraOpenedMedia: Sendable {
  let handles: [TeraPreparedMediaHandle]
  private let files: Mutex<[FileHandle]>

  init(handles: [TeraPreparedMediaHandle], files: [FileHandle]) {
    self.handles = handles
    self.files = Mutex(files)
  }

  deinit {
    close()
  }

  func close() {
    let active = files.withLock { files in
      let active = files
      files.removeAll()
      return active
    }
    // File I/O occurs outside the short ownership-transfer critical section.
    for file in active {
      try? file.close()
    }
  }
}

extension TeraOpenedMedia {
  static func open(_ values: [TeraPreparedMedia], using media: (any TeraAddMediaHandling)?) async throws -> TeraOpenedMedia {
    guard !values.isEmpty else { return TeraOpenedMedia(handles: [], files: []) }
    guard let media else {
      throw TeraRuntimeFailure.local(
        operation: "add.media.open",
        code: "ios.add.media_unavailable",
        safeMessage: "Prepared photos are unavailable on this device."
      )
    }
    return try await media.open(values)
  }
}
