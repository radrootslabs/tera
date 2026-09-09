import Darwin
import Foundation
@testable import TeraApp

enum TeraOpenedMediaCloseFixture {
  static func closeConcurrently(_ owner: TeraOpenedMedia) async {
    await withTaskGroup(of: Void.self) { group in
      for _ in 0 ..< 64 {
        group.addTask {
          for _ in 0 ..< 64 {
            owner.close()
          }
        }
      }
    }
  }
}

/// The unique fixture stays linked throughout inspection. An inode comparison
/// remains valid if another simulator task reuses a released descriptor slot.
struct TeraOpenFileProbe: Sendable {
  private let device: Int32
  private let inode: UInt64

  init(url: URL) throws {
    var value = stat()
    guard Darwin.lstat(url.path, &value) == 0 else { throw ProbeError.unavailable }
    device = value.st_dev
    inode = value.st_ino
  }

  func owns(_ descriptor: Int32) -> Bool {
    var value = stat()
    return Darwin.fstat(descriptor, &value) == 0
      && value.st_dev == device && value.st_ino == inode
  }

  var descriptorCount: Int {
    // The operating system bounds the descriptor table; no pathname is opened
    // by this scan, and descriptors belonging to other files are ignored.
    (0 ..< getdtablesize()).reduce(0) { count, descriptor in
      count + (owns(descriptor) ? 1 : 0)
    }
  }

  private enum ProbeError: Error { case unavailable }
}
