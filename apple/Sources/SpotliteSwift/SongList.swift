import SwiftUI

/// Which columns a song list shows.
struct SongColumns: OptionSet, Sendable {
    let rawValue: Int
    static let number = SongColumns(rawValue: 1 << 0)
    static let cover = SongColumns(rawValue: 1 << 1)
    static let album = SongColumns(rawValue: 1 << 2)
    static let added = SongColumns(rawValue: 1 << 3)
    static let addedBy = SongColumns(rawValue: 1 << 4)
    static let date = SongColumns(rawValue: 1 << 5)

    static let playlist: SongColumns = [.number, .cover, .album, .added]
    static let albumTracks: SongColumns = [.number]
    static let plain: SongColumns = [.number, .cover, .album]
}

/// Where a list's songs come from, for playing and editing them.
struct SongSource {
    /// The Spotify context the songs play in, when they have one.
    var context: String?
    /// A playlist the account can edit, so songs can be taken out of it.
    var editablePlaylist: String?
}

/// Music's song list: a heading row and the songs on alternating stripes.
/// Rows play on a double click or Return and offer the song's menu.
struct SongList: View {
    @Environment(AppModel.self) private var model
    var tracks: [Track]
    var columns: SongColumns = .plain
    var source = SongSource()
    var more: (() -> Void)?

    @State private var selection: Set<Track.ID> = []
    @State private var anchor: Track.ID?
    @FocusState private var focused: Bool

    var body: some View {
        LazyVStack(alignment: .leading, spacing: 0) {
            SongHeader(columns: columns)
            ForEach(tracks) { track in
                SongRow(
                    track: track,
                    columns: columns,
                    striped: track.position % 2 == 1,
                    selected: selection.contains(track.id),
                    playing: model.player.uri == track.uri,
                    paused: model.player.playing != true
                )
                .contentShape(Rectangle())
                .gesture(TapGesture(count: 2).onEnded { play(track) })
                .simultaneousGesture(TapGesture().modifiers(.command).onEnded { toggle(track) })
                .simultaneousGesture(TapGesture().modifiers(.shift).onEnded { extend(to: track) })
                .simultaneousGesture(TapGesture().onEnded { pick(track) })
                .contextMenu { SongMenu(tracks: picked(around: track), source: source) }
                .draggable(track.uri)
                .onAppear {
                    if track.position == tracks.count - 1 { more?() }
                }
            }
        }
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onKeyPress(.return) {
            guard let track = tracks.first(where: { selection.contains($0.id) }) else { return .ignored }
            play(track)
            return .handled
        }
        .onKeyPress(.upArrow) { move(-1) }
        .onKeyPress(.downArrow) { move(1) }
    }

    private func play(_ track: Track) {
        guard track.playable != false else { return }
        model.play(track, in: source.context, tracks: tracks)
    }

    private func pick(_ track: Track) {
        focused = true
        selection = [track.id]
        anchor = track.id
    }

    private func toggle(_ track: Track) {
        focused = true
        if selection.contains(track.id) {
            selection.remove(track.id)
        } else {
            selection.insert(track.id)
        }
        anchor = track.id
    }

    private func extend(to track: Track) {
        focused = true
        guard let anchor, let start = tracks.firstIndex(where: { $0.id == anchor }) else {
            pick(track)
            return
        }
        let end = track.position
        selection = Set(tracks[min(start, end)...max(start, end)].map(\.id))
    }

    private func move(_ step: Int) -> KeyPress.Result {
        let current = tracks.firstIndex { selection.contains($0.id) } ?? -1
        let next = max(0, min(tracks.count - 1, current + step))
        guard tracks.indices.contains(next) else { return .ignored }
        selection = [tracks[next].id]
        anchor = tracks[next].id
        return .handled
    }

    /// The songs a menu acts on: the selection when the row is in it, or
    /// the row alone.
    private func picked(around track: Track) -> [Track] {
        selection.contains(track.id) ? tracks.filter { selection.contains($0.id) } : [track]
    }
}

private enum Widths {
    static let number: CGFloat = 34
    static let time: CGFloat = 52
    static let added: CGFloat = 110
    static let addedBy: CGFloat = 110
    static let heart: CGFloat = 24
}

struct SongHeader: View {
    var columns: SongColumns

    var body: some View {
        HStack(spacing: 12) {
            if columns.contains(.number) { Text("#").frame(width: Widths.number) }
            Text("Title").frame(maxWidth: .infinity, alignment: .leading)
            if columns.contains(.album) {
                Text("Album").frame(maxWidth: .infinity, alignment: .leading)
            }
            if columns.contains(.addedBy) {
                Text("Added By").frame(width: Widths.addedBy, alignment: .leading)
            }
            if columns.contains(.added) {
                Text("Date Added").frame(width: Widths.added, alignment: .leading)
            }
            if columns.contains(.date) {
                Text("Date").frame(width: Widths.added, alignment: .leading)
            }
            Color.clear.frame(width: Widths.heart)
            Text("Time").frame(width: Widths.time, alignment: .trailing)
        }
        .font(.caption.weight(.semibold))
        .foregroundStyle(.secondary)
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .overlay(alignment: .bottom) { Divider() }
        .padding(.bottom, 4)
    }
}

struct SongRow: View {
    @Environment(AppModel.self) private var model
    var track: Track
    var columns: SongColumns
    var striped: Bool
    var selected: Bool
    var playing: Bool
    var paused: Bool

    @State private var hovered = false

    private var secondary: AnyShapeStyle {
        selected ? AnyShapeStyle(.white.opacity(0.85)) : AnyShapeStyle(.secondary)
    }

    var body: some View {
        HStack(spacing: 12) {
            if columns.contains(.number) {
                number.frame(width: Widths.number)
            }
            HStack(spacing: 10) {
                if columns.contains(.cover) {
                    Artwork(url: track.image, size: 36)
                }
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 5) {
                        Text(track.name)
                            .fontWeight(.medium)
                            .lineLimit(1)
                        if track.explicit {
                            Image(systemName: "e.square.fill")
                                .font(.caption)
                                .foregroundStyle(secondary)
                        }
                    }
                    if columns.contains(.cover) || !track.isEpisode {
                        ArtistLinks(track: track, style: secondary)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if columns.contains(.album) {
                Button {
                    if let id = track.album?.id { model.open(.album(id)) }
                } label: {
                    Text(track.albumName).lineLimit(1)
                }
                .buttonStyle(.link)
                .foregroundStyle(secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            if columns.contains(.addedBy) {
                Text(track.addedBy ?? "")
                    .foregroundStyle(secondary)
                    .lineLimit(1)
                    .frame(width: Widths.addedBy, alignment: .leading)
            }
            if columns.contains(.added) {
                Text(relativeDate(track.addedAt))
                    .foregroundStyle(secondary)
                    .lineLimit(1)
                    .frame(width: Widths.added, alignment: .leading)
            }
            if columns.contains(.date) {
                Text(shortDate(track.releaseDate))
                    .foregroundStyle(secondary)
                    .lineLimit(1)
                    .frame(width: Widths.added, alignment: .leading)
            }
            Button {
                model.toggleSaved(track.uri)
            } label: {
                Image(systemName: "heart")
            }
            .buttonStyle(.plain)
            .foregroundStyle(secondary)
            .opacity(hovered && !track.isEpisode ? 1 : 0)
            .frame(width: Widths.heart)
            Text(track.duration)
                .monospacedDigit()
                .foregroundStyle(secondary)
                .frame(width: Widths.time, alignment: .trailing)
        }
        .foregroundStyle(selected ? AnyShapeStyle(.white) : AnyShapeStyle(.primary))
        .opacity(track.playable == false ? 0.45 : 1)
        .padding(.horizontal, 10)
        .padding(.vertical, columns.contains(.cover) ? 5 : 8)
        .background {
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(selected
                    ? AnyShapeStyle(.tint)
                    : striped ? AnyShapeStyle(.quinary) : AnyShapeStyle(.clear))
        }
        .onHover { hovered = $0 }
    }

    @ViewBuilder private var number: some View {
        if hovered {
            Button {
                if playing { model.toggle() } else {
                    model.play(track, in: nil, tracks: nil)
                }
            } label: {
                Image(systemName: playing && !paused ? "pause.fill" : "play.fill")
            }
            .buttonStyle(.plain)
        } else if playing {
            Image(systemName: paused ? "speaker.fill" : "speaker.wave.2.fill")
                .foregroundStyle(selected ? AnyShapeStyle(.white) : AnyShapeStyle(.tint))
        } else {
            Text("\(columns == .albumTracks ? track.trackNumber ?? track.position + 1 : track.position + 1)")
                .monospacedDigit()
                .foregroundStyle(secondary)
        }
    }
}

/// A song's artists, each opening its page, or an episode's show.
struct ArtistLinks: View {
    @Environment(AppModel.self) private var model
    var track: Track
    var style: AnyShapeStyle

    var body: some View {
        HStack(spacing: 0) {
            if track.isEpisode {
                Button(track.subtitle ?? "") {
                    if let show = track.showId { model.open(.show(show)) }
                }
            } else {
                ForEach(Array(track.artists.enumerated()), id: \.offset) { index, artist in
                    if index > 0 { Text(", ") }
                    Button(artist.name) {
                        if let id = artist.id { model.open(.artist(id)) }
                    }
                }
            }
        }
        .buttonStyle(.link)
        .font(.callout)
        .foregroundStyle(style)
        .lineLimit(1)
    }
}

/// Everything a song offers on a right click.
struct SongMenu: View {
    @Environment(AppModel.self) private var model
    var tracks: [Track]
    var source: SongSource

    var body: some View {
        let first = tracks.first
        Button("Play Next") { model.addToQueue(tracks) }
        Divider()
        Menu("Add to Playlist") {
            Button("New Playlist…") {
                model.requestNewPlaylist(with: tracks)
            }
            Divider()
            ForEach(model.editablePlaylists) { playlist in
                Button(playlist.name) { model.add(tracks, to: playlist) }
            }
        }
        if let playlist = source.editablePlaylist {
            Button("Remove from This Playlist") { model.remove(tracks, from: playlist) }
        }
        if let first, tracks.count == 1, !first.isEpisode {
            Button("Save to / Remove from Liked Songs") { model.toggleSaved(first.uri) }
            Divider()
            Button("Start Radio") { model.startRadio(seed: first.uri) }
            if let album = first.album?.id {
                Button("Go to Album") { model.open(.album(album)) }
            }
            ForEach(first.artists.filter { $0.id != nil }, id: \.self) { artist in
                Button("Go to \(artist.name)") { model.open(.artist(artist.id!)) }
            }
        }
        if let first, tracks.count == 1, first.isEpisode, let show = first.showId {
            Button("Go to Podcast") { model.open(.show(show)) }
        }
        if let first, tracks.count == 1 {
            Divider()
            Button("Copy Song Link") { model.copyLink(first.uri) }
        }
    }
}

func relativeDate(_ iso: String?) -> String {
    guard let iso, !iso.hasPrefix("1970") else { return "" }
    let parser = ISO8601DateFormatter()
    parser.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    guard let date = parser.date(from: iso) ?? ISO8601DateFormatter().date(from: iso) else { return "" }
    let age = Date().timeIntervalSince(date)
    if age < 60 * 60 * 24 * 30 {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .full
        return formatter.localizedString(for: date, relativeTo: Date())
    }
    return date.formatted(date: .abbreviated, time: .omitted)
}

func shortDate(_ text: String?) -> String {
    guard let text, !text.isEmpty else { return "" }
    let formatter = DateFormatter()
    formatter.dateFormat = "yyyy-MM-dd"
    if let date = formatter.date(from: text) {
        return date.formatted(date: .abbreviated, time: .omitted)
    }
    return text
}
