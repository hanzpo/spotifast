import AppKit
import SwiftUI

@main
struct SpotliteApp: App {
    @State private var model = AppModel()

    init() {
        // Launched from a terminal by `swift run`, the process still gets a
        // Dock icon and a window of its own.
        NSApplication.shared.setActivationPolicy(.regular)
        if ProcessInfo.processInfo.environment["SPOTLITE_BACKGROUND"] == nil {
            NSApplication.shared.activate()
        }
    }

    var body: some Scene {
        WindowGroup("Spotlite") {
            ContentView()
                .environment(model)
                .frame(minWidth: 900, minHeight: 560)
                .task { await model.connect() }
        }
        .windowToolbarStyle(.unified)
        .commands {
            CommandMenu("Controls") {
                Button("Play/Pause") { model.toggle() }
                    .keyboardShortcut(.space, modifiers: [])
                Button("Next") { model.next() }
                    .keyboardShortcut(.rightArrow, modifiers: .command)
                Button("Previous") { model.previous() }
                    .keyboardShortcut(.leftArrow, modifiers: .command)
            }
        }
    }
}
