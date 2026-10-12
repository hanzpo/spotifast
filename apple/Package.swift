// swift-tools-version: 6.2
// A native SwiftUI front end for Spotlite. It drives the running Spotlite
// over its local API (src/server.rs); Spotlite keeps the session,
// playback and library.
import PackageDescription

let package = Package(
    name: "SpotliteSwift",
    platforms: [.macOS("26.0")],
    targets: [
        .executableTarget(name: "SpotliteSwift")
    ]
)
