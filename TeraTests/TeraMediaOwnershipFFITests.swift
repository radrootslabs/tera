import CryptoKit
import Darwin
import Foundation
@testable import TeraApp
import TeraKitBindings
import XCTest

final class TeraMediaOwnershipFFITests: XCTestCase {
  func testCallerCloseBeforeGeneratedConversionPreservesAdmittedBytes() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let opened = try TeraMediaFileFixture.open([fixture.media], bytes: fixture.bytes)
    opened.close()
    let saved = try await fixture.save(runtime, handle: XCTUnwrap(opened.handles.first))
    XCTAssertEqual(saved.form?.media.first?.sha256, fixture.media.sha256)
    _ = try await runtime.shutdown()
  }

  func testReusingCallerDescriptorBeforeGeneratedConversionCannotSubstituteBytes() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let original = try fixture.original()
    defer { try? original.close() }
    let handle = try TeraPreparedMediaHandle(media: fixture.media, fileDescriptor: UInt64(original.fileDescriptor))
    let replacementURL = fixture.root.appendingPathComponent("replacement")
    try Data(repeating: 0, count: fixture.bytes.count).write(to: replacementURL)
    let replacement = try FileHandle(forReadingFrom: replacementURL)
    defer { try? replacement.close() }
    // Both descriptor slots belong exclusively to this fixture. The original
    // FileHandle remains the sole closer of the atomically replaced slot.
    XCTAssertEqual(dup2(replacement.fileDescriptor, original.fileDescriptor), original.fileDescriptor)
    XCTAssertEqual(try original.readToEnd(), Data(repeating: 0, count: fixture.bytes.count))
    let saved = try await fixture.save(runtime, handle: handle)
    XCTAssertEqual(saved.form?.media.first?.sha256, fixture.media.sha256)
    _ = try await runtime.shutdown()
  }

  func testCancelledBoundedCallerCanCloseWhileLateForeignWorkRetainsItsFile() async throws {
    try await assertLateWork(timeout: false)
  }

  func testTimedOutBoundedCallerCanCloseWhileLateForeignWorkRetainsItsFile() async throws {
    try await assertLateWork(timeout: true)
  }

  func testAdmissionRejectsInvalidAndNonregularDescriptorsSynchronously() throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    XCTAssertThrowsError(try TeraPreparedMediaHandle(media: fixture.media, fileDescriptor: .max))
    let directory = Darwin.open(fixture.root.path, O_RDONLY)
    XCTAssertGreaterThanOrEqual(directory, 0)
    guard directory >= 0 else { return }
    defer { Darwin.close(directory) }
    XCTAssertThrowsError(try TeraPreparedMediaHandle(media: fixture.media, fileDescriptor: UInt64(directory)))
  }

  func testPreparedMediaSchemaIsVersionedWithoutChangingDraftSchema() async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let opened = try TeraMediaFileFixture.open([fixture.media], bytes: fixture.bytes)
    let handle = try XCTUnwrap(opened.handles.first)
    var input = fixture.input(handle)
    XCTAssertEqual(input.schemaVersion, 1)
    XCTAssertEqual(input.media[0].schemaVersion, 2)
    input.media[0].schemaVersion = 1
    do {
      _ = try await runtime.phase1SaveDraft(
        draftId: String(repeating: "31", count: 16), input: input,
        authoredAtUnixS: 1_800_000_000, expectedRevision: nil, persistedAtUnixMs: 1_800_000_000_000
      )
      XCTFail("The borrowed media schema must be rejected")
    } catch let TeraAppError.Failure(report) {
      XCTAssertEqual(report.code, "unsupported_schema_version")
    }
    _ = try await runtime.shutdown()
  }

  private func assertLateWork(timeout: Bool) async throws {
    let fixture = try MediaOwnershipFixture()
    defer { fixture.remove() }
    let runtime = try await fixture.runtime()
    let original = try fixture.original()
    let handle = try TeraPreparedMediaHandle(media: fixture.media, fileDescriptor: UInt64(original.fileDescriptor))
    let opened = TeraOpenedMedia(handles: [handle], files: [original])
    let pause = ResourceTestPause()
    let task = TeraRuntimeBoundedTask<String>(deadlineNanoseconds: timeout ? 1 : .max) {
      await pause.wait()
      // Model independently executing foreign work: caller cancellation does
      // not establish that the operation stopped or that its resource is free.
      return await Task.detached { () -> Result<String, TeraRuntimeFailure> in
        do {
          let saved = try await fixture.save(runtime, handle: handle)
          return .success(saved.form?.media.first?.sha256 ?? "missing")
        } catch {
          return .failure(TeraRuntimeFailure.local(operation: "fixture", code: "fixture.media.failed", safeMessage: "Fixture failed."))
        }
      }.value
    }
    await pause.entered.wait()
    if !timeout {
      task.cancel()
    }
    switch await task.value() {
    case .timedOut: XCTAssertTrue(timeout)
    case .cancelled: XCTAssertFalse(timeout)
    case .completed: XCTFail("The foreign work is still paused")
    }
    await TeraOpenedMediaCloseFixture.closeConcurrently(opened)
    XCTAssertNil(task.settlement())
    await pause.resume.open()
    let late = await task.settle()
    XCTAssertEqual(try late.get(), fixture.media.sha256)
    _ = try await runtime.shutdown()
  }
}

struct MediaOwnershipFixture: Sendable {
  let root: URL
  let bytes = Data([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2])
  private let publicKey = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"

  init() throws {
    root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root.appendingPathComponent("radroots/users/\(publicKey)"), withIntermediateDirectories: true)
    try bytes.write(to: root.appendingPathComponent("original.png"))
  }

  var media: TeraPreparedMedia {
    let hash = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    return TeraPreparedMedia(opaqueReference: "media:\(hash)", remoteURL: nil, sha256: hash,
                             mediaType: "image/png", byteSize: UInt64(bytes.count), width: 2, height: 2,
                             alt: "Synthetic ownership fixture", preparedAtUnixSeconds: 1_800_000_000)
  }

  func original() throws -> FileHandle {
    try FileHandle(forReadingFrom: root.appendingPathComponent("original.png"))
  }

  func remove() {
    try? FileManager.default.removeItem(at: root)
  }

  func runtime() async throws -> TeraRuntime {
    let runtime = try await TeraRuntime(applicationSupportDirectory: root.path, publicKeyHex: publicKey,
                                        sourceGenerationHex: String(repeating: "04", count: 32),
                                        sourceGenerationCreatedAtUnixMs: 1_800_000_000_000, protectedData: .available)
    try runtime.configureBlossom(hostKind: .simulator, endpointAuthority: .loopbackDevelopment,
                                 primaryOrigin: "http://127.0.0.1:3000", fallbackOrigins: [])
    return runtime
  }

  func save(_ runtime: TeraRuntime, handle: TeraPreparedMediaHandle) async throws -> FfiDraftStatusRecord {
    try await runtime.phase1SaveDraft(draftId: String(repeating: "31", count: 16), input: input(handle),
                                      authoredAtUnixS: 1_800_000_000, expectedRevision: nil, persistedAtUnixMs: 1_800_000_000_000)
  }

  func input(_ handle: TeraPreparedMediaHandle) -> FfiAddDraftInput {
    FfiAddDraftInput(schemaVersion: 1, commandType: .createPhotoUpdate, content: "Synthetic ownership fixture",
                     identifier: nil, title: nil, summary: nil, location: nil, eventTiming: nil,
                     eventStartDate: nil, eventEndDate: nil, eventStartUnixS: nil, eventEndUnixS: nil,
                     eventTimezone: nil, priceAmount: nil, currency: nil, unit: nil, quantity: nil,
                     foodPublishedAtUnixS: nil, foodStatus: nil, media: [handle.generatedValue])
  }
}
