import AppKit
import Foundation

/// Finds the engine that serves this app, starting it when it isn't
/// running. The engine, SwiftSpot's Rust core, keeps the Spotify session
/// and plays the audio; it ships inside this app and runs out of sight.
@MainActor
enum Connector {
    /// The engine started from here, which quits when this app does.
    private static var engine: Process?

    /// The engine inside this app's bundle.
    static var bundledEngine: URL? {
        let url = Bundle.main.bundleURL
            .appendingPathComponent("Contents/Helpers/SwiftSpot Engine.app/Contents/MacOS/SwiftSpot")
        return FileManager.default.isExecutableFile(atPath: url.path) ? url : nil
    }

    /// An engine installed elsewhere, for runs from the build folder.
    static var installedEngine: URL? {
        let url = URL(fileURLWithPath: "/Applications/SwiftSpot.app/Contents/Helpers/SwiftSpot Engine.app/Contents/MacOS/SwiftSpot")
        return FileManager.default.isExecutableFile(atPath: url.path) ? url : nil
    }

    static func connect() async throws -> Client {
        if let client = try? Client() { return client }
        // A demo or another socket named on the command line is never
        // started from here.
        guard ProcessInfo.processInfo.environment["SWIFTSPOT_SOCKET"] == nil,
              let executable = bundledEngine ?? installedEngine
        else {
            return try Client()
        }
        if engine?.isRunning != true {
            let process = Process()
            process.executableURL = executable
            // No window and no Dock icon, and gone as soon as this app is.
            process.arguments = ["--background", "--exit-with-parent"]
            process.standardOutput = FileHandle.nullDevice
            process.standardError = FileHandle.nullDevice
            try process.run()
            engine = process
        }
        // The engine takes a moment to open its session and its socket.
        var failure: Error = Client.Failure.notRunning(Client.defaultPath)
        for _ in 0..<80 {
            try? await Task.sleep(for: .milliseconds(250))
            do {
                return try Client()
            } catch {
                failure = error
            }
            if engine?.isRunning == false { break }
        }
        throw failure
    }
}
