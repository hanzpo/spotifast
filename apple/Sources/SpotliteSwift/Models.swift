import Foundation

struct ArtistRef: Codable, Hashable, Sendable {
    var id: String?
    var name: String
}

struct AlbumRef: Codable, Hashable, Sendable {
    var id: String?
    var name: String?
}

/// A song or a podcast episode in a list.
struct Track: Codable, Hashable, Identifiable, Sendable {
    var kind: String
    var uri: String
    var name: String
    var artists: [ArtistRef]
    var album: AlbumRef?
    var durationMs: Int
    var explicit: Bool
    var image: String?
    var addedAt: String?
    var addedBy: String?
    var trackNumber: Int?
    var discNumber: Int?
    var subtitle: String?
    var showId: String?
    var releaseDate: String?
    var description: String?
    var resumeMs: Int?
    var played: Bool?
    var playable: Bool?

    /// A song can sit in a list twice; the row's position keeps them apart.
    var position: Int = 0
    var id: String { "\(position):\(uri)" }

    enum CodingKeys: String, CodingKey {
        case kind, uri, name, artists, album, durationMs, explicit, image, addedAt, addedBy,
             trackNumber, discNumber, subtitle, showId, releaseDate, description, resumeMs,
             played, playable
    }

    var isEpisode: Bool { kind == "episode" }
    var artistNames: String {
        isEpisode ? (subtitle ?? "") : artists.map(\.name).joined(separator: ", ")
    }
    var albumName: String { album?.name ?? "" }
    var duration: String { formatTime(ms: durationMs) }
}

/// Anything shown as a card in a grid or shelf.
struct Card: Codable, Hashable, Identifiable, Sendable {
    var kind: String
    var id: String
    var uri: String
    var name: String
    var subtitle: String?
    var image: String?

    var route: Route? {
        switch kind {
        case "playlist": .playlist(id)
        case "album": .album(id)
        case "artist": .artist(id)
        case "show": .show(id)
        case "user": .user(id)
        case "liked": .liked
        default: nil
        }
    }

    var round: Bool { kind == "artist" || kind == "user" }
}

struct Shelf: Codable, Hashable, Identifiable, Sendable {
    var id: String
    var title: String
    var cards: [Card]?
    var tracks: [Track]?
    var seeAll: String?
}

struct Owner: Codable, Hashable, Sendable {
    var id: String?
    var name: String
}

struct Playlist: Codable, Hashable, Identifiable, Sendable {
    var id: String
    var uri: String
    var name: String
    var description: String?
    var owner: Owner
    var image: String?
    var cover: String?
    var total: Int?
    var editable: Bool?
    var owned: Bool?
    var saved: Bool?
    var collaborative: Bool?
    var isPublic: Bool?

    enum CodingKeys: String, CodingKey {
        case id, uri, name, description, owner, image, cover, total, editable, owned, saved,
             collaborative
        case isPublic = "public"
    }
}

/// A row of the sidebar's playlists: a playlist, or a folder of them.
struct SidebarRow: Codable, Hashable, Identifiable, Sendable {
    var kind: String
    var id: String
    var name: String
    var depth: Int
    var playlist: Playlist?
    var children: [SidebarRow]?
}

struct PlaylistList: Codable, Sendable {
    var loading: Bool
    var error: String?
    var playlists: [Playlist]?
    var rows: [SidebarRow]?
    var likedContext: String?
}

/// A page of songs with a header: a playlist, an album, Liked Songs, a
/// radio, Top Songs.
struct SongsPage: Codable, Sendable {
    var loading: Bool
    var error: String?
    var page: String?
    var title: String?
    var kind: String?
    var owner: String?
    var ownerRoute: String?
    var artists: [ArtistRef]?
    var description: String?
    var image: String?
    var cover: String?
    var facts: [String]?
    var context: String?
    var uri: String?
    var saved: Bool?
    var editable: Bool?
    var owned: Bool?
    var total: Int?
    var more: Bool?
    var tracks: [Track]?
    var showAlbum: Bool?
    var showAdded: Bool?
    var stationUris: [String]?
    var seed: String?
}

struct ArtistPage: Codable, Sendable {
    struct Artist: Codable, Sendable {
        var id: String
        var uri: String
        var name: String
        var image: String?
        var cover: String?
        var followers: Int?
        var genres: [String]?
    }
    var loading: Bool
    var error: String?
    var artist: Artist?
    var following: Bool?
    var topTracks: [Track]?
    var albums: [Card]?
    var singles: [Card]?
    var appearsOn: [Card]?
    var compilations: [Card]?
    var related: [Card]?
}

struct ShowPage: Codable, Sendable {
    struct Show: Codable, Sendable {
        var id: String
        var uri: String
        var name: String
        var publisher: String?
        var description: String?
        var image: String?
        var cover: String?
        var total: Int?
    }
    var loading: Bool
    var error: String?
    var show: Show?
    var saved: Bool?
    var more: Bool?
    var episodes: [Track]?
}

struct UserPage: Codable, Sendable {
    struct User: Codable, Sendable {
        var id: String
        var name: String
        var image: String?
        var followers: Int?
    }
    var loading: Bool
    var error: String?
    var user: User?
    var playlists: [Card]?
}

struct CardsPage: Codable, Sendable {
    var loading: Bool
    var error: String?
    var more: Bool?
    var cards: [Card]
}

struct HomePage: Codable, Sendable {
    var loading: Bool
    var greeting: String?
    var shelves: [Shelf]
}

struct SearchResults: Codable, Sendable {
    var query: String
    var loading: Bool
    var error: String?
    var tracks: [Track]
    var artists: [Card]
    var albums: [Card]
    var playlists: [Card]
    var shows: [Card]
    var episodes: [Track]
}

struct QueueState: Codable, Sendable {
    var loading: Bool
    var current: Track?
    var queued: [Track]
    var next: [Track]
    var context: String?
    var reorderable: Bool?
    var clearable: Bool?
}

struct Device: Codable, Hashable, Identifiable, Sendable {
    var id: String
    var name: String
    var type: String
    var active: Bool
    var volume: Int?
    var local: Bool?

    var symbol: String {
        switch type.lowercased() {
        case "computer": "laptopcomputer"
        case "smartphone": "iphone"
        case "tablet": "ipad"
        case "speaker": "hifispeaker"
        case "tv": "tv"
        case "avr", "stb": "hifireceiver"
        case "automobile": "car"
        case "gameconsole": "gamecontroller"
        case "castvideo", "castaudio": "airplayaudio"
        case "smartwatch": "applewatch"
        default: "speaker.wave.2"
        }
    }
}

struct DevicesState: Codable, Sendable {
    var loading: Bool
    var devices: [Device]
}

struct LyricsLine: Codable, Hashable, Sendable {
    var timeMs: Int?
    var text: String
}

struct LyricsState: Codable, Sendable {
    var uri: String?
    var loading: Bool
    var error: String?
    var synced: Bool
    var instrumental: Bool?
    var lines: [LyricsLine]
}

struct Hello: Codable, Sendable {
    struct User: Codable, Sendable {
        var id: String
        var name: String
        var image: String?
        var product: String?
    }
    var app: String
    var version: String
    var signedIn: Bool
    var auth: String?
    var authError: String?
    var signInUrl: String?
    var user: User?
}

struct PlayerState: Codable, Equatable, Sendable {
    var hasSong: Bool
    var uri: String?
    var title: String?
    var subtitle: String?
    var artists: [ArtistRef]?
    var album: AlbumRef?
    var showId: String?
    var image: String?
    var cover: String?
    var durationMs: Int?
    var positionMs: Int?
    var playing: Bool?
    var loading: Bool?
    var shuffle: Bool?
    var `repeat`: String?
    var volume: Int?
    var canSetVolume: Bool?
    var local: Bool?
    var device: String?
    var isEpisode: Bool?
    var saved: Bool?
    var context: String?
    var playingFrom: String?
    var playingFromPage: String?

    static let empty = PlayerState(hasSong: false)
}

/// A setting the server lets a client change.
struct Setting: Codable, Hashable, Identifiable, Sendable {
    struct Choice: Codable, Hashable, Sendable {
        var value: String
        var label: String
    }
    var key: String
    var label: String
    var detail: String?
    var kind: String
    var value: SettingValue
    var choices: [Choice]?
    var min: Double?
    var max: Double?
    var step: Double?
    var group: String

    var id: String { key }
}

enum SettingValue: Codable, Hashable, Sendable {
    case bool(Bool)
    case number(Double)
    case text(String)

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let bool = try? container.decode(Bool.self) {
            self = .bool(bool)
        } else if let number = try? container.decode(Double.self) {
            self = .number(number)
        } else {
            self = .text(try container.decode(String.self))
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .bool(let value): try container.encode(value)
        case .number(let value): try container.encode(value)
        case .text(let value): try container.encode(value)
        }
    }

    var json: any Sendable {
        switch self {
        case .bool(let value): value
        case .number(let value): value
        case .text(let value): value
        }
    }
}

struct SettingsState: Codable, Sendable {
    var settings: [Setting]
    var eqPresets: [String]?
}

/// Somewhere the window can go.
enum Route: Hashable, Sendable {
    case home
    case search
    case liked
    case albums
    case artists
    case podcasts
    case topSongs
    case playlist(String)
    case album(String)
    case artist(String)
    case show(String)
    case user(String)
    case radio(String)

    /// The server's name for the page, as `page.more` and the pages take it.
    var page: String {
        switch self {
        case .home: "home"
        case .search: "search"
        case .liked: "liked"
        case .albums: "albums"
        case .artists: "artists"
        case .podcasts: "podcasts"
        case .topSongs: "top-songs"
        case .playlist(let id): "playlist:\(id)"
        case .album(let id): "album:\(id)"
        case .artist(let id): "artist:\(id)"
        case .show(let id): "show:\(id)"
        case .user(let id): "user:\(id)"
        case .radio(let seed): "radio:\(seed)"
        }
    }

    init?(page: String) {
        let parts = page.split(separator: ":", maxSplits: 1).map(String.init)
        switch (parts.first, parts.count > 1 ? parts[1] : nil) {
        case ("home", _): self = .home
        case ("search", _): self = .search
        case ("liked", _): self = .liked
        case ("albums", _): self = .albums
        case ("artists", _): self = .artists
        case ("podcasts", _): self = .podcasts
        case ("top-songs", _): self = .topSongs
        case ("playlist", let id?): self = .playlist(id)
        case ("album", let id?): self = .album(id)
        case ("artist", let id?): self = .artist(id)
        case ("show", let id?): self = .show(id)
        case ("user", let id?): self = .user(id)
        case ("radio", let seed?): self = .radio(seed)
        default: return nil
        }
    }
}

func formatTime(ms: Int) -> String {
    let seconds = max(ms, 0) / 1000
    let hours = seconds / 3600
    let minutes = seconds / 60 % 60
    let rest = seconds % 60
    return hours > 0
        ? String(format: "%d:%02d:%02d", hours, minutes, rest)
        : String(format: "%d:%02d", minutes, rest)
}
