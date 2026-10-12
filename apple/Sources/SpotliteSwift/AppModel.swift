import AppKit
import Foundation
import Observation

@Observable
@MainActor
final class AppModel {
    enum Connection: Equatable {
        case connecting
        case connected
        case failed(String)
    }

    var connection = Connection.connecting
    var hello: Hello?

    // MARK: Navigation

    /// The sidebar's choice, the root of the page stack.
    var root: Route = .home {
        didSet {
            if root != oldValue {
                path = []
                forwardStack = []
            }
        }
    }
    /// Pages opened on top of the root, as Back and Forward walk them.
    var path: [Route] = []
    private var forwardStack: [Route] = []
    var route: Route { path.last ?? root }

    func open(_ route: Route) {
        guard route != self.route else { return }
        forwardStack = []
        path.append(route)
    }

    func back() {
        guard let last = path.popLast() else { return }
        forwardStack.append(last)
    }

    func forward() {
        guard let next = forwardStack.popLast() else { return }
        path.append(next)
    }

    var canGoBack: Bool { !path.isEmpty }
    var canGoForward: Bool { !forwardStack.isEmpty }

    // MARK: Library

    var playlists: [Playlist] = []
    var sidebarRows: [SidebarRow] = []
    var likedContext: String?
    /// Bumped after anything that changes what pages show, so they reload.
    var revision = 0

    // MARK: Search

    var searchText = ""

    // MARK: Player

    var player = PlayerState.empty
    /// When `player` arrived, so the position can count on between reports.
    var playerAt = Date()
    /// A seek being dragged, as a fraction.
    var scrubbing: Double?
    var volumeDragging: Int?

    // MARK: Panels

    enum Panel: String { case queue, lyrics }
    var panel: Panel?
    var queue: QueueState?
    var lyrics: LyricsState?
    var devices: [Device] = []
    var showDevices = false

    // MARK: Messages

    var toast: String?
    private var toastTask: Task<Void, Never>?

    private(set) var client: Client?

    // MARK: Connecting

    init() {
        // For screenshots and debugging: the page and panel to start on.
        let environment = ProcessInfo.processInfo.environment
        if let page = environment["SPOTLITE_ROUTE"], let route = Route(page: page) {
            if case .playlist = route { root = route } else if [.home, .search, .recentlyAdded, .allPlaylists, .liked, .albums, .artists, .podcasts, .episodes, .topSongs].contains(route) {
                root = route
            } else {
                path = [route]
            }
        }
        if let query = environment["SPOTLITE_SEARCH"] { searchText = query }
        panel = environment["SPOTLITE_PANEL"].flatMap(Panel.init(rawValue:))
    }

    func connect() async {
        connection = .connecting
        do {
            let client = try await Connector.connect()
            client.onEvent = { [weak self] event, data in
                self?.handle(event: event, data: data)
            }
            client.onClose = { [weak self] in
                self?.client = nil
                self?.connection = .failed("Spotlite stopped.")
                Task { [weak self] in
                    try? await Task.sleep(for: .seconds(2))
                    await self?.connect()
                }
            }
            self.client = client
            hello = try await client.call("hello", as: Hello.self)
            connection = .connected
            if hello?.signedIn == true {
                await refreshAll()
            }
        } catch {
            connection = .failed(error.localizedDescription)
        }
    }

    func refreshAll() async {
        guard let client else { return }
        if let state = try? await client.call("player.state", as: PlayerState.self) {
            setPlayer(state)
        }
        await loadPlaylists()
        revision += 1
    }

    private func handle(event: String, data: Data) {
        switch event {
        case "player":
            if let state = try? JSONDecoder.api.decode(PlayerState.self, from: data) {
                let changedSong = state.uri != player.uri
                setPlayer(state)
                if changedSong {
                    Task {
                        if panel == .queue { await loadQueue() }
                        if panel == .lyrics { await loadLyrics() }
                    }
                }
            }
        case "auth":
            Task {
                guard let client else { return }
                hello = try? await client.call("hello", as: Hello.self)
                if hello?.signedIn == true { await refreshAll() }
            }
        case "library":
            Task {
                await loadPlaylists()
                revision += 1
            }
        case "queue":
            if panel == .queue { Task { await loadQueue() } }
        case "toast":
            struct Message: Decodable { var message: String }
            if let toast = try? JSONDecoder.api.decode(Message.self, from: data) {
                show(toast.message)
            }
        default:
            break
        }
    }

    private func setPlayer(_ state: PlayerState) {
        // A drag on the volume keeps the slider where the hand put it.
        var state = state
        if let volume = volumeDragging { state.volume = volume }
        player = state
        playerAt = Date()
    }

    func show(_ message: String) {
        toast = message
        toastTask?.cancel()
        toastTask = Task {
            try? await Task.sleep(for: .seconds(3))
            if !Task.isCancelled { toast = nil }
        }
    }

    // MARK: Requests

    func call<T: Decodable>(_ method: String, _ params: [String: Any] = [:], as type: T.Type) async throws -> T {
        guard let client else { throw Client.Failure.closed }
        return try await client.call(method, params, as: type)
    }

    /// Sends a command whose reply only matters if it failed.
    func send(_ method: String, _ params: [String: Any] = [:], done: String? = nil) {
        guard let client else { return }
        Task {
            do {
                _ = try await client.call(method, params)
                if let done { show(done) }
            } catch {
                show(error.localizedDescription)
            }
        }
    }

    func loadPlaylists() async {
        for attempt in 0..<60 {
            guard let list = try? await call("library.playlists", as: PlaylistList.self) else { return }
            // A reload in progress or one that failed keeps the list shown.
            if let fresh = list.playlists { playlists = fresh }
            if let rows = list.rows { sidebarRows = rows }
            likedContext = list.likedContext ?? likedContext
            if !list.loading { return }
            try? await Task.sleep(for: .milliseconds(attempt < 10 ? 250 : 800))
        }
    }

    var editablePlaylists: [Playlist] { playlists.filter { $0.editable == true } }

    // MARK: Playback

    /// Plays a song from a list. A list `reordered` from its context's own
    /// order, sorted or filtered, plays in the order shown.
    func play(_ track: Track, in context: String?, tracks: [Track]? = nil, reordered: Bool = false) {
        var params: [String: Any] = ["uri": track.uri]
        if let context, reordered, let tracks {
            params["context"] = context
            params["uris"] = tracks.map(\.uri)
            params["index"] = tracks.firstIndex { $0.id == track.id } ?? 0
        } else if let context, context.hasPrefix("spotify:station:") {
            // A radio plays the songs it shows, under the station's name.
            let list = tracks ?? [track]
            params["context"] = context
            params["uris"] = list.map(\.uri)
            params["index"] = list.firstIndex { $0.id == track.id } ?? 0
        } else if let context {
            params["context"] = context
            params["index"] = track.position
        } else if track.isEpisode {
            if let resume = track.resumeMs, track.played != true { params["resume_ms"] = resume }
        } else if let tracks {
            // A list with no Spotify context of its own (search results,
            // an artist's top songs) plays as the songs on screen.
            params["uris"] = tracks.map(\.uri)
            params["index"] = tracks.firstIndex { $0.id == track.id } ?? 0
        }
        player.uri = track.uri
        player.title = track.name
        player.subtitle = track.artistNames
        player.artists = track.artists
        player.album = track.album
        player.isEpisode = track.isEpisode
        player.showId = track.showId
        player.image = track.image
        player.saved = nil
        player.durationMs = track.durationMs
        player.positionMs = 0
        player.playing = true
        player.hasSong = true
        playerAt = Date()
        send("player.play", params)
        confirmPlayer()
    }

    /// Asks for the player once the engine has had time to act, so a play
    /// that failed, and sent no news, does not leave its guess on show.
    private func confirmPlayer() {
        Task {
            try? await Task.sleep(for: .seconds(2.5))
            if let state = try? await call("player.state", as: PlayerState.self) {
                setPlayer(state)
            }
        }
    }

    func play(context: String, shuffle: Bool? = nil, uris: [String]? = nil) {
        if let shuffle {
            player.shuffle = shuffle
            send("player.shuffle", ["on": shuffle])
        }
        var params: [String: Any] = ["context": context]
        // A radio has no list of its own on Spotify: it plays the songs
        // it shows, shuffled from a random one when asked.
        if let uris, !uris.isEmpty {
            params["uris"] = uris
            let start = shuffle == true ? Int.random(in: 0..<uris.count) : 0
            params["uri"] = uris[start]
            params["index"] = start
        }
        send("player.play", params)
        confirmPlayer()
    }

    func toggle() {
        guard player.hasSong else { return }
        if player.playing == true {
            player.positionMs = position(at: Date())
        }
        player.playing?.toggle()
        playerAt = Date()
        send("player.toggle")
    }

    func next() { send("player.next") }
    func previous() { send("player.previous") }

    func toggleShuffle() {
        let on = !(player.shuffle ?? false)
        player.shuffle = on
        send("player.shuffle", ["on": on])
    }

    func cycleRepeat() {
        let next = switch player.repeat {
        case "off", nil: "context"
        case "context": "track"
        default: "off"
        }
        player.repeat = next
        send("player.repeat", ["mode": next])
    }

    func toggleSaved(_ uri: String) {
        if uri == player.uri { player.saved = !(player.saved ?? false) }
        send("library.toggle", ["uri": uri])
    }

    func seek(to fraction: Double) {
        let position = Int(fraction * Double(player.durationMs ?? 0))
        seek(ms: position)
    }

    func seek(ms: Int) {
        player.positionMs = max(0, min(ms, player.durationMs ?? ms))
        playerAt = Date()
        send("player.seek", ["position_ms": player.positionMs ?? 0])
    }

    func skip(seconds: Int) {
        seek(ms: position(at: Date()) + seconds * 1000)
    }

    func setVolume(_ percent: Int, done: Bool) {
        player.volume = percent
        volumeDragging = done ? nil : percent
        send("player.volume", ["percent": percent, "done": done])
    }

    func changeVolume(by delta: Int) {
        setVolume(max(0, min(100, (player.volume ?? 0) + delta)), done: true)
    }

    func toggleMute() { send("player.mute") }

    /// Where the song is now, counted on from the last report.
    func position(at date: Date) -> Int {
        let reported = player.positionMs ?? 0
        guard player.playing == true, player.loading != true else { return reported }
        let elapsed = Int(date.timeIntervalSince(playerAt) * 1000)
        return min(reported + elapsed, player.durationMs ?? Int.max)
    }

    // MARK: Queue

    func togglePanel(_ panel: Panel) {
        self.panel = self.panel == panel ? nil : panel
        Task {
            if self.panel == .queue { await loadQueue() }
            if self.panel == .lyrics { await loadLyrics() }
        }
    }

    func loadQueue() async {
        for attempt in 0..<20 {
            guard let state = try? await call("queue.get", as: QueueState.self) else { return }
            queue = state
            if !state.loading { return }
            try? await Task.sleep(for: .milliseconds(attempt < 5 ? 200 : 600))
        }
    }

    func addToQueue(_ tracks: [Track]) {
        send(
            "queue.add",
            ["songs": tracks.map { ["uri": $0.uri, "name": $0.name] }],
            done: tracks.count == 1 ? "Added to queue" : "Added \(tracks.count) songs to queue")
        if panel == .queue { Task { try? await Task.sleep(for: .milliseconds(400)); await loadQueue() } }
    }

    func playQueued(at index: Int, queued: Bool) {
        send("queue.play", ["index": index, "queued": queued])
        Task { try? await Task.sleep(for: .milliseconds(400)); await loadQueue() }
    }

    /// Moves a song queued by hand; `to` is the row it goes before.
    func moveQueued(from: Int, to: Int) {
        if var queued = queue?.queued {
            queued.move(fromOffsets: IndexSet(integer: from), toOffset: to)
            queue?.queued = queued
        }
        send("queue.move", ["from": from, "to": to])
    }

    func clearQueue() {
        queue?.queued = []
        send("queue.clear")
    }

    // MARK: Lyrics

    func loadLyrics() async {
        for attempt in 0..<30 {
            guard let state = try? await call("lyrics.get", as: LyricsState.self) else { return }
            lyrics = state
            if !state.loading { return }
            try? await Task.sleep(for: .milliseconds(attempt < 5 ? 200 : 600))
        }
    }

    // MARK: Devices

    func loadDevices() async {
        send("devices.refresh")
        for attempt in 0..<10 {
            guard let state = try? await call("devices.list", as: DevicesState.self) else { return }
            devices = state.devices
            if !state.loading && attempt > 1 { return }
            try? await Task.sleep(for: .milliseconds(400))
        }
    }

    func transfer(to device: Device) {
        for index in devices.indices { devices[index].active = devices[index].id == device.id }
        send("devices.transfer", ["id": device.id])
    }

    // MARK: Library edits

    /// Songs waiting for the New Playlist sheet, which opens while set.
    var newPlaylistSongs: [Track]?
    var editingPlaylist: Playlist?

    func requestNewPlaylist(with tracks: [Track] = []) { newPlaylistSongs = tracks }

    func createPlaylist(named name: String, isPublic: Bool, with tracks: [Track] = []) {
        send(
            "playlist.create",
            ["name": name, "public": isPublic, "uris": tracks.map(\.uri)],
            done: "Created “\(name)”")
    }

    func add(_ tracks: [Track], to playlist: Playlist) {
        // Songs go with what the server needs to show their rows at once;
        // episodes go by URI.
        let songs = tracks.filter { !$0.isEpisode }.map(\.described)
        let episodes = tracks.filter(\.isEpisode).map(\.uri)
        send(
            "playlist.add",
            ["id": playlist.id, "items": songs, "uris": episodes],
            done: "Added to \(playlist.name)")
        revision += 1
    }

    func remove(_ tracks: [Track], from playlistID: String) {
        send(
            "playlist.remove",
            ["id": playlistID, "items": tracks.map { ["uri": $0.uri, "position": $0.position] }])
        revision += 1
    }

    /// Moves a song one place up or down its playlist.
    func move(_ track: Track, in playlistID: String, by step: Int) {
        let from = track.spotifyPosition ?? track.position
        // Spotify inserts before `to`, counted before the song leaves.
        let to = step < 0 ? from - 1 : from + 2
        guard to >= 0 else { return }
        send("playlist.move", ["id": playlistID, "from": from, "to": to])
        revision += 1
    }

    func rename(_ playlist: Playlist, to name: String, description: String?) {
        var params: [String: Any] = ["id": playlist.id, "name": name]
        if let description { params["description"] = description }
        send("playlist.edit", params)
    }

    func delete(_ playlist: Playlist) {
        if route == .playlist(playlist.id) { back() }
        if root == .playlist(playlist.id) { root = .home }
        send("playlist.delete", ["id": playlist.id], done: "Removed “\(playlist.name)”")
    }

    func toggleLibrary(uri: String) {
        send("library.toggle", ["uri": uri])
        revision += 1
    }

    func follow(artist id: String, follow: Bool) {
        send("artist.follow", ["id": id, "follow": follow])
        revision += 1
    }

    func startRadio(seed: String) { open(.radio(seed)) }

    func copyLink(_ uri: String) {
        let parts = uri.split(separator: ":")
        let link = parts.count >= 3 ? "https://open.spotify.com/\(parts[1])/\(parts[2])" : uri
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(link, forType: .string)
        show("Link copied")
    }

    // MARK: Account

    func signIn() { send("auth.sign_in") }
    func signOut() { send("auth.sign_out") }
}
