import AppKit
import SwiftUI

@main
struct SpotliteApp: App {
    @State private var model = AppModel()

    init() {
        // Run from a terminal by `swift run`, the process still gets a
        // Dock icon and a window of its own.
        NSApplication.shared.setActivationPolicy(.regular)
        if ProcessInfo.processInfo.environment["SPOTLITE_BACKGROUND"] == nil {
            NSApplication.shared.activate()
        }
    }

    var body: some Scene {
        Window("Spotlite", id: "main") {
            ContentView()
                .environment(model)
                .frame(minWidth: 1080, minHeight: 640)
                .task { await model.connect() }
        }
        .windowToolbarStyle(.unified)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Playlist…") { model.requestNewPlaylist() }
                    .keyboardShortcut("n")
            }
            CommandGroup(after: .sidebar) {
                Button("Back") { model.back() }
                    .keyboardShortcut("[")
                    .disabled(!model.canGoBack)
                Button("Forward") { model.forward() }
                    .keyboardShortcut("]")
                    .disabled(!model.canGoForward)
                Divider()
                Button("Lyrics") { model.togglePanel(.lyrics) }
                    .keyboardShortcut("u", modifiers: [.command, .option])
                Button("Playing Next") { model.togglePanel(.queue) }
                    .keyboardShortcut("u")
                Divider()
            }
            CommandMenu("Controls") {
                Button(model.player.playing == true ? "Pause" : "Play") { model.toggle() }
                    .keyboardShortcut(.space, modifiers: .option)
                Button("Next") { model.next() }
                    .keyboardShortcut(.rightArrow)
                Button("Previous") { model.previous() }
                    .keyboardShortcut(.leftArrow)
                Divider()
                Button("Skip Forward 10 Seconds") { model.skip(seconds: 10) }
                    .keyboardShortcut(.rightArrow, modifiers: [.command, .option])
                Button("Skip Back 10 Seconds") { model.skip(seconds: -10) }
                    .keyboardShortcut(.leftArrow, modifiers: [.command, .option])
                Divider()
                Button("Increase Volume") { model.changeVolume(by: 5) }
                    .keyboardShortcut(.upArrow)
                Button("Decrease Volume") { model.changeVolume(by: -5) }
                    .keyboardShortcut(.downArrow)
                Button("Mute") { model.toggleMute() }
                Divider()
                Toggle("Shuffle", isOn: Binding(
                    get: { model.player.shuffle == true },
                    set: { _ in model.toggleShuffle() }))
                Button("Repeat: \(repeatLabel)") { model.cycleRepeat() }
                    .keyboardShortcut("r")
                Divider()
                Button("Go to Current Song") {
                    if let album = model.player.album?.id {
                        model.open(.album(album))
                    } else if let show = model.player.showId {
                        model.open(.show(show))
                    }
                }
                .keyboardShortcut("l")
                Button(model.player.saved == true ? "Remove from Liked Songs" : "Save to Liked Songs") {
                    if let uri = model.player.uri { model.toggleSaved(uri) }
                }
                .disabled(model.player.uri == nil)
            }
        }

        Settings {
            SettingsView()
                .environment(model)
        }
    }

    private var repeatLabel: String {
        switch model.player.repeat {
        case "context": "All"
        case "track": "One"
        default: "Off"
        }
    }
}
