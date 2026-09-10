import Foundation
import ImageIO

nonisolated enum TeraMediaThumbnailFailure: Error {
  case corrupt
  case resourceLimit
}

nonisolated enum TeraMediaThumbnail {
  /// The caller owns this task and its worker slot through decode completion.
  static func prepare(_ artifact: TeraVerifiedMediaArtifact,
                      limits: TeraMediaPresentationLimits) async throws -> CGImage
  {
    let task = Task.detached(priority: .utility) {
      try decode(artifact, limits: limits)
    }
    return try await withTaskCancellationHandler {
      try await task.value
    } onCancel: {
      task.cancel()
    }
  }

  private static func decode(_ artifact: TeraVerifiedMediaArtifact,
                             limits: TeraMediaPresentationLimits) throws -> CGImage
  {
    assert(!Thread.isMainThread, "Raster decoding must stay off the UI thread")
    try Task.checkCancellation()
    guard limits.accepts(byteCount: artifact.bytes.count, width: Int(artifact.width),
                         height: Int(artifact.height)) else { throw TeraMediaThumbnailFailure.resourceLimit }
    guard let source = CGImageSourceCreateWithData(artifact.bytes as CFData,
                                                   [kCGImageSourceShouldCache: false] as CFDictionary),
      let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
      let width = properties[kCGImagePropertyPixelWidth] as? Int,
      let height = properties[kCGImagePropertyPixelHeight] as? Int,
      width == Int(artifact.width), height == Int(artifact.height)
    else {
      throw TeraMediaThumbnailFailure.corrupt
    }
    guard limits.accepts(byteCount: artifact.bytes.count, width: width, height: height) else {
      throw TeraMediaThumbnailFailure.resourceLimit
    }
    let options: [CFString: Any] = [
      kCGImageSourceCreateThumbnailFromImageAlways: true,
      kCGImageSourceThumbnailMaxPixelSize: limits.thumbnailDimension,
      kCGImageSourceCreateThumbnailWithTransform: true,
      kCGImageSourceShouldCacheImmediately: true,
    ]
    guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary),
      image.width <= limits.thumbnailDimension, image.height <= limits.thumbnailDimension
    else {
      throw TeraMediaThumbnailFailure.corrupt
    }
    try Task.checkCancellation()
    return image
  }
}
