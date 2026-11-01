// swift-tools-version:5.9
// Janus for macOS: the native workbench. It runs the Rust orchestrator (`janus`) it ships with,
// talks to it over the same HTTP API the web page uses, and renders every view in SwiftUI.
// Build and package with `just app` / `just dmg` (macos/scripts/package.sh).
import PackageDescription

let package = Package(
    name: "Janus",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(name: "Janus", path: "Sources/Janus"),
        .testTarget(name: "JanusTests", dependencies: ["Janus"], path: "Tests/JanusTests"),
    ]
)
