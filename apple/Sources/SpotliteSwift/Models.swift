import Foundation

struct ArtistRef: Codable, Hashable {
    var id: String?
    var name: String
}

struct AlbumRef: Codable, Hashable {
    var id: String?
    var name: String?
}

struct Track: Codable, Hashable, Identifiable {
    var kind: String
    var uri: String
    var name: String
    var artists: [ArtistRef]
    var album: AlbumRef?
    var durationMs: Int
    var explicit: Bool
    var image: String?
    var addedAt: String?
    var trackNumber: Int?

    /// A song can sit in a list twice; the row's position keeps them apart.
    var position: Int = 0
    var id: String { "\(position):\(uri)" }

    enum CodingKeys: String, CodingKey {
        case kind, uri, name, artists, album, durationMs, explicit, image, addedAt, trackNumber
    }

    var artistNames: String { artists.map(\.name).joined(separator: ", ") }
    var albumName: String { album?.name ?? "" }
    var duration: String { formatTime(ms: durationMs) }
}

struct Owner: Codable, Hashable {
    var id: String?
    var name: String
}

struct Playlist: Codable, Hashable, Identifiable {
    var id: String
    var uri: String
    var name: String
    var description: String?
    var owner: Owner
    var image: String?
    var cover: String?
    var total: Int?
}

struct PlaylistList: Codable {
    var loading: Bool
    var error: String?
    var playlists: [Playlist]?
    var likedContext: String?
}

struct PlaylistDetail: Codable {
    var loading: Bool
    var error: String?
    var playlist: Playlist?
    var total: Int?
    var more: Bool?
    var tracks: [Track]?
}

struct LikedSongs: Codable {
    var loading: Bool
    var error: String?
    var total: Int?
    var more: Bool?
    var context: String?
    var tracks: [Track]
}

struct Hello: Codable {
    struct User: Codable {
        var id: String
        var name: String
        var image: String?
    }
    var app: String
    var version: String
    var signedIn: Bool
    var user: User?
}

struct PlayerState: Codable, Equatable {
    var hasSong: Bool
    var uri: String?
    var title: String?
    var subtitle: String?
    var artists: [ArtistRef]?
    var album: AlbumRef?
    var image: String?
    var cover: String?
    var durationMs: Int?
    var positionMs: Int?
    var playing: Bool?
    var shuffle: Bool?
    var `repeat`: String?
    var volume: Int?
    var local: Bool?
    var device: String?
    var saved: Bool?
    var context: String?

    static let empty = PlayerState(hasSong: false)
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
