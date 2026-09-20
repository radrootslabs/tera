import Foundation
import RadrootsKit

#if DEBUG
  actor TeraRemoteQualificationMediaPicker: RadrootsMediaPicker {
    private let roots: RadrootsAppleFileRoots
    private let file: RadrootsFileReference

    init(roots: RadrootsAppleFileRoots, file: RadrootsFileReference) {
      self.roots = roots
      self.file = file
    }

    func currentSupport() async throws -> RadrootsMediaPickerSupport {
      try RadrootsMediaPickerSupport(
        importAvailable: true,
        cameraCaptureAvailable: false,
        supportedImportKinds: [.image],
        supportedCaptureKinds: [],
        multipleSelectionSupported: false
      )
    }

    func importMedia(
      _ request: RadrootsMediaImportRequest
    ) async throws -> RadrootsMediaImportResult {
      guard request.allowedMediaKinds == [.image], request.selectionLimit >= 1 else {
        throw RadrootsCaptureIntakeError.invalidRequest
      }
      try RadrootsAppleFileAccess(roots: roots).write(
        .inline(TeraRemoteQualificationEnvironment.mediaFixtureData()),
        to: file
      )
      let url = try roots.resolvedURL(for: file)
      let values = try url.resourceValues(
        forKeys: [.fileSizeKey, .isRegularFileKey, .isSymbolicLinkKey]
      )
      guard values.isRegularFile == true,
        values.isSymbolicLink != true,
        let size = values.fileSize,
        (1 ... 40 * 1024 * 1024).contains(size)
      else {
        throw RadrootsCaptureIntakeError.unavailable
      }
      let asset = try RadrootsMediaAsset(
        source: .libraryImport,
        kind: .image,
        file: file,
        mediaType: "image/png",
        suggestedFilename: "input.png",
        sizeBytes: UInt64(size),
        capturedAt: Date()
      )
      return try RadrootsMediaImportResult(items: [asset])
    }

    func captureMedia(
      _: RadrootsMediaCaptureRequest
    ) async throws -> RadrootsMediaCaptureResult {
      throw RadrootsCaptureIntakeError.unavailable
    }
  }
#endif
