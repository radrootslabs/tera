import RadrootsKit

enum TeraCameraAccess: Sendable, Equatable, CaseIterable {
  case authorized, notDetermined, denied, restricted, unavailable

  static func current() -> Self {
    switch RadrootsApplePermissionStatusAdapters.live.cameraStatus() {
    case .authorized: .authorized
    case .notDetermined: .notDetermined
    case .denied: .denied
    case .restricted: .restricted
    default: .unavailable
    }
  }

  var canCapture: Bool {
    self == .authorized || self == .notDetermined
  }

  var label: String {
    switch self {
    case .authorized: "Ready"
    case .notDetermined: "Permission requested when used"
    case .denied: "Access denied"
    case .restricted: "Restricted on this device"
    case .unavailable: "Unavailable"
    }
  }

  var guidance: String {
    switch self {
    case .authorized: "Camera access is available."
    case .notDetermined: "Camera access is requested only when you choose Camera."
    case .denied: "Camera access is off. You can enable it in Settings, then check access again."
    case .restricted: "Camera access is restricted on this device. A device administrator or parental controls may manage this restriction."
    case .unavailable: "A camera is unavailable on this device."
    }
  }
}

struct TeraAddMediaSupport: Sendable, Equatable {
  let library: Bool
  let camera: Bool
  let cameraAccess: TeraCameraAccess

  init(library: Bool, camera: Bool, cameraAccess: TeraCameraAccess = .authorized) {
    self.library = library
    self.camera = camera && cameraAccess.canCapture
    self.cameraAccess = camera ? cameraAccess : .unavailable
  }

  static let unavailable = Self(library: false, camera: false)
}
