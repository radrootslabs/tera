import Foundation

/// Presentation budgets only. Eviction never mutates an authored artifact.
struct TeraMediaPresentationLimits: Sendable, Equatable {
  let encodedBytes: Int
  let sourcePixels: Int
  let thumbnailDimension: Int
  let cacheBytes: Int
  let cacheEntries: Int
  let workers: Int
  let queuedRequests: Int
  let visibilityEntries: Int

  static let standard: Self = {
    guard let limits = Self(encodedBytes: 8 * 1024 * 1024, sourcePixels: 16_000_000,
                            thumbnailDimension: 1024, cacheBytes: 32 * 1024 * 1024, cacheEntries: 64,
                            workers: 2, queuedRequests: 16, visibilityEntries: 4096)
    else {
      preconditionFailure("Bundled media presentation limits must be valid")
    }
    return limits
  }()

  init?(encodedBytes: Int, sourcePixels: Int, thumbnailDimension: Int, cacheBytes: Int,
        cacheEntries: Int, workers: Int, queuedRequests: Int, visibilityEntries: Int)
  {
    guard (1 ... 8 * 1024 * 1024).contains(encodedBytes),
      (1 ... 16_000_000).contains(sourcePixels), (1 ... 1024).contains(thumbnailDimension),
      (1 ... 32 * 1024 * 1024).contains(cacheBytes), (1 ... 64).contains(cacheEntries),
      (1 ... 2).contains(workers), (1 ... 16).contains(queuedRequests),
      (1 ... 4096).contains(visibilityEntries), workers + queuedRequests <= cacheEntries else { return nil }
    self.encodedBytes = encodedBytes
    self.sourcePixels = sourcePixels
    self.thumbnailDimension = thumbnailDimension
    self.cacheBytes = cacheBytes
    self.cacheEntries = cacheEntries
    self.workers = workers
    self.queuedRequests = queuedRequests
    self.visibilityEntries = visibilityEntries
  }

  func accepts(byteCount: Int, width: Int, height: Int) -> Bool {
    byteCount > 0 && byteCount <= encodedBytes && width > 0 && height > 0
      && width <= sourcePixels / height
  }
}
