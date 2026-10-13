// swift-tools-version: 6.2
// A native SwiftUI front end for SwiftSpot. It drives the running SwiftSpot
// over its local API (src/server.rs); SwiftSpot keeps the session,
// playback and library.
import PackageDescription

let package = Package(
    name: "SwiftSpot",
    platforms: [.macOS("26.0")],
    targets: [
        .executableTarget(name: "SwiftSpot")
    ]
)
