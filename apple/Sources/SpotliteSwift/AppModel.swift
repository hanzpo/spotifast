import Foundation
import Observation

/// What the sidebar can show.
enum SidebarItem: Hashable {
    case liked
    case playlist(String)
}

/// The list on screen: its header and its songs.
struct Detail {
    var title: String
    var owner: String?
    var description: String?
    var cover: String?
    var facts: String
    var context: String?
    var tracks: [Track]
    var loading: Bool
    var error: String?
}

@Observable
@MainActor
final class AppModel {
    var connection: String?
    var hello: Hello?
    var playlists: [Playlist] = []
    var likedContext: String?
    var selection: SidebarItem? = .liked {
        didSet { if selection != oldValue { Task { await loadDetail() } } }
    }
    var detail: Detail?
    var player = PlayerState.empty
    /// When `player` arrived, so the position can count on between reports.
    var playerAt = Date()
    /// A seek being dragged, as a fraction.
    var scrubbing: Double?

    private var client: Client?

    func connect() async {
        do {
            let client = try Client()
            client.onEvent = { [weak self] event, data in
                guard event == "player", let self,
                      let state = try? JSONDecoder.api.decode(PlayerState.self, from: data) else { return }
                self.player = state
                self.playerAt = Date()
            }
            client.onClose = { [weak self] in
                self?.connection = "Spotlite stopped. Open it and relaunch this app."
            }
            self.client = client
            connection = nil
            hello = try await client.call("hello", as: Hello.self)
            player = try await client.call("player.state", as: PlayerState.self)
            playerAt = Date()
            await loadPlaylists()
            await loadDetail()
        } catch {
            connection = error.localizedDescription
        }
    }

    /// Asks again while Spotlite is still fetching, as its API expects.
    private func poll<T: Decodable>(
        _ method: String, _ params: [String: Any] = [:], as type: T.Type,
        loading: (T) -> Bool, update: (T) -> Void
    ) async {
        guard let client else { return }
        for attempt in 0..<60 {
            guard let value = try? await client.call(method, params, as: T.self) else { return }
            update(value)
            if !loading(value) { return }
            try? await Task.sleep(for: .milliseconds(attempt < 10 ? 200 : 600))
        }
    }

    func loadPlaylists() async {
        await poll("library.playlists", as: PlaylistList.self, loading: \.loading) { list in
            playlists = list.playlists ?? []
            likedContext = list.likedContext
        }
    }

    func loadDetail() async {
        switch selection {
        case .liked:
            await poll("library.liked", as: LikedSongs.self, loading: \.loading) { liked in
                guard selection == .liked else { return }
                detail = Detail(
                    title: "Liked Songs",
                    owner: hello?.user?.name,
                    facts: "PLAYLIST · \(liked.total ?? liked.tracks.count) SONGS",
                    context: liked.context,
                    tracks: numbered(liked.tracks),
                    loading: liked.loading,
                    error: liked.error)
            }
        case .playlist(let id):
            await poll("playlist.get", ["id": id], as: PlaylistDetail.self, loading: \.loading) { page in
                guard selection == .playlist(id) else { return }
                let header = page.playlist ?? playlists.first { $0.id == id }
                let tracks = page.tracks ?? []
                let minutes = tracks.reduce(0) { $0 + $1.durationMs } / 60_000
                detail = Detail(
                    title: header?.name ?? "",
                    owner: header?.owner.name,
                    description: header?.description,
                    cover: header?.cover,
                    facts: "PLAYLIST · \(page.total ?? tracks.count) SONGS · \(minutes) MIN",
                    context: header?.uri,
                    tracks: numbered(tracks),
                    loading: page.loading,
                    error: page.error)
            }
        case nil:
            detail = nil
        }
    }

    private func numbered(_ tracks: [Track]) -> [Track] {
        tracks.enumerated().map { index, track in
            var track = track
            track.position = index
            return track
        }
    }

    // MARK: Playback

    func play(_ track: Track) {
        var params: [String: Any] = ["uri": track.uri]
        if let context = detail?.context {
            params["context"] = context
            params["index"] = track.position
        }
        client?.send("player.play", params)
    }

    func playDetail(shuffle: Bool) {
        guard let context = detail?.context else { return }
        client?.send("player.shuffle", ["on": shuffle])
        client?.send("player.play", ["context": context])
    }

    func toggle() {
        player.playing?.toggle()
        playerAt = Date()
        client?.send("player.toggle")
    }

    func next() { client?.send("player.next") }
    func previous() { client?.send("player.previous") }

    func toggleShuffle() {
        let on = !(player.shuffle ?? false)
        player.shuffle = on
        client?.send("player.shuffle", ["on": on])
    }

    func cycleRepeat() {
        let next = switch player.repeat {
        case "off": "context"
        case "context": "track"
        default: "off"
        }
        player.repeat = next
        client?.send("player.repeat", ["mode": next])
    }

    func like() {
        player.saved = !(player.saved ?? false)
        client?.send("player.like")
    }

    func seek(to fraction: Double) {
        let position = Int(fraction * Double(player.durationMs ?? 0))
        player.positionMs = position
        playerAt = Date()
        client?.send("player.seek", ["position_ms": position])
    }

    func setVolume(_ percent: Int) {
        player.volume = percent
        client?.send("player.volume", ["percent": percent])
    }

    /// Where the song is now, counted on from the last report.
    func position(at date: Date) -> Int {
        let reported = player.positionMs ?? 0
        guard player.playing == true else { return reported }
        let elapsed = Int(date.timeIntervalSince(playerAt) * 1000)
        return min(reported + elapsed, player.durationMs ?? Int.max)
    }
}
