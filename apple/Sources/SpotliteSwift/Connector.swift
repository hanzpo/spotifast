import AppKit
import Foundation

/// Finds the Spotlite that serves this app, starting it when it isn't
/// running: it keeps the Spotify session and plays the audio.
@MainActor
enum Connector {
    /// The installed Spotlite, which this app starts in the background.
    static var serverApp: URL? {
        let workspace = NSWorkspace.shared
        if let url = workspace.urlForApplication(withBundleIdentifier: "rocks.spotifast.Spotifast") {
            return url
        }
        let installed = URL(fileURLWithPath: "/Applications/Spotlite.app")
        return FileManager.default.fileExists(atPath: installed.path) ? installed : nil
    }

    static func connect() async throws -> Client {
        if let client = try? Client() { return client }
        // A demo or another socket named on the command line is never
        // started from here.
        guard ProcessInfo.processInfo.environment["SPOTLITE_SOCKET"] == nil, let app = serverApp else {
            return try Client()
        }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = false
        configuration.addsToRecentItems = false
        configuration.createsNewApplicationInstance = false
        configuration.arguments = ["--background"]
        _ = try? await NSWorkspace.shared.openApplication(at: app, configuration: configuration)
        // Spotlite takes a moment to open its session and its socket.
        var failure: Error = Client.Failure.notRunning(Client.defaultPath)
        for _ in 0..<60 {
            try? await Task.sleep(for: .milliseconds(250))
            do {
                return try Client()
            } catch {
                failure = error
            }
        }
        throw failure
    }
}
