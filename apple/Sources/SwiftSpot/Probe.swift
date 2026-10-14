import AppKit
import SwiftUI

/// A window AppKit never pulls back onto the screen.
private final class OffscreenWindow: NSWindow {
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect { frameRect }
}

/// Probe runs (`SWIFTSPOT_PROBE`) draw the interface out of sight, for
/// checking it without disturbing whoever is at the Mac: the window sits
/// beneath the desktop and the app never becomes active.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    static var model: AppModel?
    private var window: NSWindow?

    /// The Dock menu, as Music's: the song, and the transport.
    func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        guard let model = Self.model else { return nil }
        let menu = NSMenu()
        if let title = model.player.title {
            let song = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            song.isEnabled = false
            menu.addItem(song)
            if let subtitle = model.player.subtitle, !subtitle.isEmpty {
                let artist = NSMenuItem(title: subtitle, action: nil, keyEquivalent: "")
                artist.isEnabled = false
                menu.addItem(artist)
            }
            menu.addItem(.separator())
        }
        for (title, selector) in [
            (model.player.playing == true ? "Pause" : "Play", #selector(togglePlay)),
            ("Next", #selector(next)),
            ("Previous", #selector(previous)),
        ] {
            let item = NSMenuItem(title: title, action: selector, keyEquivalent: "")
            item.target = self
            menu.addItem(item)
        }
        return menu
    }

    @objc private func togglePlay() { Self.model?.toggle() }
    @objc private func next() { Self.model?.next() }
    @objc private func previous() { Self.model?.previous() }

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard SwiftSpotApp.probing, let model = Self.model else { return }
        // SWIFTSPOT_PROBE_SIZE, as WIDTHxHEIGHT, tries other window sizes.
        let parts = (ProcessInfo.processInfo.environment["SWIFTSPOT_PROBE_SIZE"] ?? "1400x880")
            .split(separator: "x").compactMap { Double($0) }
        let size = parts.count == 2 ? NSSize(width: parts[0], height: parts[1]) : NSSize(width: 1400, height: 880)
        let window = OffscreenWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered, defer: false)
        window.title = "SwiftSpot"
        window.toolbarStyle = .unified
        window.isReleasedWhenClosed = false
        // SWIFTSPOT_PROBE_VIEW draws the Settings window or the Mini Player.
        if ProcessInfo.processInfo.environment["SWIFTSPOT_PROBE_VIEW"] == "mini" {
            window.contentView = NSHostingView(rootView: MiniPlayer()
                .environment(model)
                .task { await model.connect() })
        } else if ProcessInfo.processInfo.environment["SWIFTSPOT_PROBE_VIEW"] == "settings" {
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
        // SWIFTSPOT_SNAPSHOT draws the window into a PNG at that path after
        // a few seconds, and quits: it works with the screen locked, where
        // a window capture can't, though without the window server's glass.
        if let path = ProcessInfo.processInfo.environment["SWIFTSPOT_SNAPSHOT"] {
            Task { @MainActor in
                try? await Task.sleep(for: .seconds(5))
                if let view = window.contentView,
                   let image = view.bitmapImageRepForCachingDisplay(in: view.bounds) {
                    view.cacheDisplay(in: view.bounds, to: image)
                    try? image.representation(using: .png, properties: [:])?
                        .write(to: URL(fileURLWithPath: path))
                }
                NSApp.terminate(nil)
            }
        }
    }
}
