// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "radroots_ios_app",
    platforms: [
        .iOS(.v18),
    ],
    products: [
        .library(name: "RadrootsApp", targets: ["RadrootsApp"]),
    ],
    dependencies: [
        .package(
            url: "https://github.com/radrootslabs/apple_kit.git",
            revision: "86f5548e9e00c2b7ad3c1d6837bff8bc41bd0f24"
        ),
    ],
    targets: [
        .binaryTarget(
            name: "RadrootsFFI",
            path: "Radroots/Frameworks/RadrootsFFI.xcframework"
        ),
        .target(
            name: "RadrootsKitBindings",
            dependencies: ["RadrootsFFI"],
            path: "Radroots/Generated",
            swiftSettings: [
                .swiftLanguageMode(.v5),
            ]
        ),
        .target(
            name: "RadrootsApp",
            dependencies: [
                "RadrootsKitBindings",
                .product(name: "RadrootsKit", package: "apple_kit"),
            ],
            path: "Radroots",
            exclude: [
                "App/App.swift",
                "Config",
                "Frameworks",
                "Generated",
                "Info.plist",
                "radroots.xcconfig",
            ],
            sources: [
                "App/AppEntry.swift",
                "App/RadrootsAppDelegate.swift",
                "App/RadrootsAppModel.swift",
                "App/RadrootsProvider.swift",
                "App/RadrootsRemoteQualification.swift",
                "App/RadrootsRootShell.swift",
                "Runtime/RadrootsAddMediaCoordinator.swift",
                "Runtime/RadrootsGeneratedRuntimeBackend.swift",
                "Runtime/RadrootsLifecycleCoordinator.swift",
                "Runtime/RadrootsRuntimeClient.swift",
                "Runtime/RadrootsRuntimeModels.swift",
                "State/RadrootsAddStore.swift",
                "State/RadrootsConfigurationStore.swift",
                "State/RadrootsIdentityStore.swift",
                "State/RadrootsMediaStore.swift",
                "State/RadrootsSessionStore.swift",
                "State/RadrootsSupportingStores.swift",
                "State/RadrootsTodayStore.swift",
                "Views/RadrootsAddView.swift",
                "Views/RadrootsSupportingViews.swift",
                "Views/RadrootsTodayView.swift",
                "Views/RuntimeStatusView.swift",
            ],
            resources: [
                .process("Resources/PrivacyInfo.xcprivacy"),
            ]
        ),
        .testTarget(
            name: "RadrootsAppTests",
            dependencies: ["RadrootsApp"],
            path: "RadrootsTests"
        ),
        .testTarget(
            name: "RadrootsAppPublicAPITests",
            dependencies: ["RadrootsApp"],
            path: "RadrootsPublicAPITests"
        ),
    ],
    swiftLanguageModes: [.v6]
)
