// swift-tools-version:5.9
// Janus for macOS: the workbench. It runs the Rust orchestrator (`janus`) it ships with, talks to
// it over its HTTP API, and renders every view in SwiftUI.
// Build and package with `just app` / `just dmg` (frontend/scripts/package.sh).
import PackageDescription

let package = Package(
    name: "Janus",
    defaultLocalization: "zh-Hans",
    platforms: [.macOS("15.0")],
    targets: [
        .executableTarget(
            name: "Janus",
            path: "Sources/Janus",
            // Keep the language tags intact on SwiftPM versions that lowercase processed localizations.
            resources: [.copy("en.lproj"), .copy("zh-Hans.lproj")]
        ),
        .testTarget(name: "JanusTests", dependencies: ["Janus"], path: "Tests/JanusTests"),
    ]
)
