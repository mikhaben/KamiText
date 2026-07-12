// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "KamiTextKit",
    platforms: [
        .iOS(.v17),
        .macCatalyst(.v17),
        .macOS(.v14)
    ],
    products: [
        .library(name: "KamiTextKit", targets: ["KamiTextKit"])
    ],
    targets: [
        .binaryTarget(name: "KamiCore", path: "../KamiCore.xcframework"),
        .target(name: "KamiTextKit", dependencies: ["KamiCore"]),
        .executableTarget(name: "KamiDemoMac", dependencies: ["KamiTextKit"]),
        .testTarget(name: "KamiTextKitTests", dependencies: ["KamiTextKit"])
    ]
)
