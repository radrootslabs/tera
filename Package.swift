// swift-tools-version: 6.0

import PackageDescription

let package = Package(
  name: "tera",
  defaultLocalization: "en",
  platforms: [
    .iOS(.v18),
  ],
  products: [
    .library(name: "TeraApp", targets: ["TeraApp"]),
  ],
  dependencies: [
    .package(
      url: "https://github.com/radrootslabs/apple_kit.git",
      revision: "35aedb6b54ff645b663fecff26082b3e91fcb232"
    ),
  ],
  targets: [
    .binaryTarget(
      name: "TeraFFI",
      path: "Tera/Frameworks/TeraFFI.xcframework"
    ),
    .target(
      name: "TeraKitBindings",
      dependencies: ["TeraFFI"],
      path: "Tera/Generated",
      swiftSettings: [
        .swiftLanguageMode(.v5),
      ]
    ),
    .target(
      name: "TeraApp",
      dependencies: [
        "TeraKitBindings",
        .product(name: "RadrootsKit", package: "apple_kit"),
      ],
      path: "Tera",
      exclude: [
        "App/App.swift",
        "Config",
        "Frameworks",
        "Generated",
        "Info.plist",
        "tera.xcconfig",
      ],
      sources: [
        "App/AppEntry.swift",
        "App/TeraAppDelegate.swift",
        "App/TeraAppModel.swift",
        "App/TeraProductStores.swift",
        "App/TeraProvider.swift",
        "App/TeraRemoteQualification.swift",
        "App/TeraRemoteQualificationEvidence.swift",
        "App/TeraRootShell.swift",
        "App/TeraTodayNavigation.swift",
        "Runtime/TeraAddMediaCoordinator.swift",
        "Runtime/TeraBackgroundUploadRequest.swift",
        "Runtime/TeraCheckedTime.swift",
        "Runtime/TeraErrorRecovery.swift",
        "Runtime/TeraGeneratedRuntimeBackend.swift",
        "Runtime/TeraLifecycleCoordinator.swift",
        "Runtime/TeraLifecycleBridge.swift",
        "Runtime/TeraOpenedMedia.swift",
        "Runtime/TeraPreparedMediaHandle.swift",
        "Runtime/TeraRuntimeBoundedTask.swift",
        "Runtime/TeraRuntimeClient.swift",
        "Runtime/TeraRuntimeModels.swift",
        "Runtime/TeraRuntimeInvalidation.swift",
        "Runtime/TeraGeneratedInvalidation.swift",
        "Runtime/TeraRuntimeResourceCreation.swift",
        "Runtime/TeraRuntimeResourceTask.swift",
        "Runtime/TeraRuntimeShutdownTask.swift",
        "Runtime/TeraSessionGeneration.swift",
        "Runtime/TeraUserMessageClassifier.swift",
        "Runtime/TeraUserMessages.swift",
        "State/TeraAddStore.swift",
        "State/TeraAddStartupSnapshot.swift",
        "State/TeraAddPresentation.swift",
        "State/TeraStoreObservation.swift",
        "State/TeraConfigurationStore.swift",
        "State/TeraIdentityStore.swift",
        "State/TeraMediaStore.swift",
        "State/TeraSessionStore.swift",
        "State/TeraSessionBootstrap.swift",
        "State/TeraSupportingStores.swift",
        "State/TeraSettingsStore.swift",
        "State/TeraTodayStore.swift",
        "State/TeraTodayPresentation.swift",
        "Views/TeraAddView.swift",
        "Views/TeraSupportingViews.swift",
        "Views/TeraTodayView.swift",
        "Views/TeraTodayStatusView.swift",
        "Views/RuntimeStatusView.swift",
      ],
      resources: [
        .process("Resources/PrivacyInfo.xcprivacy"),
        .process("Resources/en.lproj/Localizable.strings"),
      ]
    ),
    .testTarget(
      name: "TeraAppTests",
      dependencies: ["TeraApp"],
      path: "TeraTests"
    ),
    .testTarget(
      name: "TeraAppPublicAPITests",
      dependencies: ["TeraApp"],
      path: "TeraPublicAPITests"
    ),
  ],
  swiftLanguageModes: [.v6]
)
