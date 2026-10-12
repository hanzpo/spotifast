import AppKit
import SwiftUI

/// A window AppKit never pulls back onto the screen.
private final class OffscreenWindow: NSWindow {
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect { frameRect }
}

/// Probe runs (`SPOTLITE_PROBE`) draw the interface out of sight, for
/// checking it without disturbing whoever is at the Mac: the window sits
/// beneath the desktop and the app never becomes active.
@MainActor
final class ProbeDelegate: NSObject, NSApplicationDelegate {
    static var model: AppModel?
    private var window: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard SpotliteApp.probing, let model = Self.model else { return }
        // SPOTLITE_PROBE_SIZE, as WIDTHxHEIGHT, tries other window sizes.
        let parts = (ProcessInfo.processInfo.environment["SPOTLITE_PROBE_SIZE"] ?? "1400x880")
            .split(separator: "x").compactMap { Double($0) }
        let size = parts.count == 2 ? NSSize(width: parts[0], height: parts[1]) : NSSize(width: 1400, height: 880)
        let window = OffscreenWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered, defer: false)
        window.title = "Spotlite"
        window.toolbarStyle = .unified
        window.isReleasedWhenClosed = false
        // SPOTLITE_PROBE_VIEW=settings draws the Settings window instead.
        if ProcessInfo.processInfo.environment["SPOTLITE_PROBE_VIEW"] == "settings" {
            window.contentView = NSHostingView(rootView: SettingsView()
                .environment(model)
                .task { await model.connect() })
        } else {
            window.contentView = NSHostingView(rootView: ContentView()
                .environment(model)
                .task { await model.connect() })
        }
        // On the screen, so the window server draws it, glass and all, but
        // beneath the desktop picture, out of the switcher and Mission
        // Control, and deaf to the mouse.
        window.level = NSWindow.Level(rawValue: Int(CGWindowLevelForKey(.desktopWindow)) - 1)
        window.collectionBehavior = [.stationary, .ignoresCycle, .transient, .canJoinAllSpaces]
        window.ignoresMouseEvents = true
        window.setFrameOrigin(.zero)
        window.orderFrontRegardless()
        self.window = window
    }
}
