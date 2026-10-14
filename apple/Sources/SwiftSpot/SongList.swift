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
    /// The list is sorted or filtered: it plays in the order shown, and
    /// its songs cannot move up or down the playlist.
    var reordered = false
    /// How many rows the editable playlist holds in Spotify's copy.
    var total: Int?
}

/// A column the list is sorted by.
struct SongSort: Equatable {
    enum Column { case title, artist, album, added, duration }
    var column: Column
    var ascending = true

    func apply(_ tracks: [Track]) -> [Track] {
        let sorted = tracks.sorted { a, b in
            switch column {
            case .title: a.name.localizedStandardCompare(b.name) == .orderedAscending
            case .artist: a.artistNames.localizedStandardCompare(b.artistNames) == .orderedAscending
            case .album:
                a.albumName == b.albumName
                    ? (a.trackNumber ?? 0) < (b.trackNumber ?? 0)
                    : a.albumName.localizedStandardCompare(b.albumName) == .orderedAscending
            case .added: (a.addedAt ?? "") < (b.addedAt ?? "")
            case .duration: a.durationMs < b.durationMs
            }
        }
        return ascending ? sorted : sorted.reversed()
    }
}

/// Music's song list: a heading row and the songs on alternating stripes.
/// Rows play on a double click or Return and offer the song's menu.
struct SongList: View {
    @Environment(AppModel.self) private var model
    var tracks: [Track]
    var columns: SongColumns = .plain
    var source = SongSource()
    var more: (() -> Void)?
    /// When set, the headings sort the list.
    var sort: Binding<SongSort?>?

    @State private var selection: Set<Track.ID> = []
    @State private var anchor: Track.ID?
    @FocusState private var focused: Bool

    var body: some View {
        LazyVStack(alignment: .leading, spacing: 0) {
            SongHeader(columns: columns, sort: sort)
            ForEach(tracks) { track in
                SongRow(
                    track: track,
                    columns: columns,
                    striped: track.position % 2 == 1,
                    selected: selection.contains(track.id),
                    playing: model.player.uri == track.uri,
                    paused: model.player.playing != true,
                    source: source,
                    play: { play(track) }
                )
                .contentShape(Rectangle())
                .gesture(TapGesture(count: 2).onEnded { play(track) })
                .simultaneousGesture(TapGesture().modifiers(.command).onEnded { toggle(track) })
                .simultaneousGesture(TapGesture().modifiers(.shift).onEnded { extend(to: track) })
                .simultaneousGesture(TapGesture().onEnded { pick(track) })
                .contextMenu { SongMenu(tracks: picked(around: track), source: source) }
                .draggable(dragPayload(track))
                .dropDestination(for: String.self) { items, _ in reorder(items, onto: track) }
                .onAppear {
                    if track.id == tracks.last?.id { more?() }
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

    /// A row dragged out of a playlist this account edits says where it
    /// came from, so dropping it on another row moves it there. Anything
    /// else that takes a drop reads only the song's URI before the `#`.
    private func dragPayload(_ track: Track) -> String {
        guard let playlist = source.editablePlaylist, !source.reordered else { return track.uri }
        return "\(track.uri)#\(playlist)#\(track.spotifyPosition ?? track.position)"
    }

    private func reorder(_ items: [String], onto target: Track) -> Bool {
        guard let playlist = source.editablePlaylist, !source.reordered,
              let parts = items.first?.split(separator: "#").map(String.init),
              parts.count == 3, parts[1] == playlist, let from = Int(parts[2])
        else { return false }
        let to = target.spotifyPosition ?? target.position
        guard from != to else { return false }
        // Spotify places the song before `to`, counted before it leaves.
        model.send("playlist.move", ["id": playlist, "from": from, "to": to > from ? to + 1 : to])
        model.revision += 1
        return true
    }

    private func play(_ track: Track) {
        guard track.playable != false else { return }
        model.play(track, in: source.context, tracks: tracks, reordered: source.reordered)
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
        guard let end = tracks.firstIndex(where: { $0.id == track.id }) else { return }
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
    static let number: CGFloat = 40
    static let time: CGFloat = 52
    static let added: CGFloat = 110
    static let addedBy: CGFloat = 110
    static let heart: CGFloat = 24
}

struct SongHeader: View {
    var columns: SongColumns
    var sort: Binding<SongSort?>?

    var body: some View {
        HStack(spacing: 12) {
            if columns.contains(.number) {
                heading("#", nil).frame(width: Widths.number)
            }
            heading("Title", .title).frame(maxWidth: .infinity, alignment: .leading)
            if columns.contains(.album) {
                heading("Album", .album).frame(maxWidth: .infinity, alignment: .leading)
            }
            if columns.contains(.addedBy) {
                Text("Added By").frame(width: Widths.addedBy, alignment: .leading)
            }
            if columns.contains(.added) {
                heading("Date Added", .added).frame(width: Widths.added, alignment: .leading)
            }
            if columns.contains(.date) {
                Text("Date").frame(width: Widths.added, alignment: .leading)
            }
            Color.clear.frame(width: Widths.heart * 2 + 12)
            heading("Time", .duration).frame(width: Widths.time, alignment: .trailing)
        }
        .font(.caption.weight(.semibold))
        .foregroundStyle(.secondary)
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .overlay(alignment: .bottom) { Divider() }
        .padding(.bottom, 4)
    }

    /// A heading that sorts by its column: once ascending, again
    /// descending, a third time back to the list's own order. `#` always
    /// goes back to that order.
    @ViewBuilder
    private func heading(_ title: String, _ column: SongSort.Column?) -> some View {
        if let sort {
            Button {
                guard let column else {
                    sort.wrappedValue = nil
                    return
                }
                switch sort.wrappedValue {
                case let current? where current.column == column && current.ascending:
                    sort.wrappedValue = SongSort(column: column, ascending: false)
                case let current? where current.column == column:
                    sort.wrappedValue = nil
                default:
                    sort.wrappedValue = SongSort(column: column)
                }
            } label: {
                HStack(spacing: 3) {
                    Text(title)
                    if let current = sort.wrappedValue, current.column == column {
                        Image(systemName: current.ascending ? "chevron.up" : "chevron.down")
                            .font(.caption2.weight(.bold))
                    }
                }
                .foregroundStyle(sort.wrappedValue?.column == column && column != nil
                    ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
            }
            .buttonStyle(.plain)
        } else {
            Text(title)
        }
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
    var source = SongSource()
    /// Plays the row as a double click does: in its list, from here.
    var play: () -> Void = {}

    @State private var hovered = false
    /// The heart as just clicked, until the list comes back with it.
    @State private var likedNow: Bool?

    private var isLiked: Bool { likedNow ?? track.saved ?? false }

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
            // Filled for a liked song, always; an outline to like one on
            // the row under the pointer.
            Button {
                let liked = !isLiked
                likedNow = liked
                model.setSaved([track], liked)
            } label: {
                Image(systemName: isLiked ? "heart.fill" : "heart")
            }
            .buttonStyle(.plain)
            .foregroundStyle(isLiked && !selected ? AnyShapeStyle(.tint) : secondary)
            .opacity(!track.isEpisode && (hovered || isLiked) ? 1 : 0)
            // Episodes have no heart: an invisible one mustn't take clicks.
            .allowsHitTesting(!track.isEpisode)
            .help(isLiked ? "Remove from Liked Songs" : "Add to Liked Songs")
            .frame(width: Widths.heart)
            // Music's More button, on the row under the pointer.
            Menu {
                SongMenu(tracks: [track], source: source)
            } label: {
                Image(systemName: "ellipsis")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .foregroundStyle(secondary)
            .opacity(hovered ? 1 : 0)
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
        // The list's own answer replaces the click's guess once it comes;
        // a like that fails changes the library, which reloads the list.
        .onChange(of: track.saved) { likedNow = nil }
    }

    @ViewBuilder private var number: some View {
        if hovered {
            Button {
                if playing { model.toggle() } else { play() }
            } label: {
                Image(systemName: playing && !paused ? "pause.fill" : "play.fill")
            }
            .buttonStyle(.plain)
        } else if playing {
            Image(systemName: paused ? "speaker.fill" : "speaker.wave.2.fill")
                .foregroundStyle(selected ? AnyShapeStyle(.white) : AnyShapeStyle(.tint))
        } else {
            // As Music numbers rows: no thousands separator, on one line.
            Text(verbatim: String(columns == .albumTracks ? track.trackNumber ?? track.position + 1 : track.position + 1))
                .monospacedDigit()
                .lineLimit(1)
                .minimumScaleFactor(0.7)
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
            if let first, tracks.count == 1, !source.reordered {
                // By the row's place in Spotify's copy, which counts rows
                // the list leaves out.
                let place = first.spotifyPosition ?? first.position
                Button("Move Up") { model.move(first, in: playlist, by: -1) }
                    .disabled(place == 0)
                Button("Move Down") { model.move(first, in: playlist, by: 1) }
                    .disabled(source.total.map { place + 1 >= $0 } ?? false)
            }
            Button("Remove from This Playlist") { model.remove(tracks, from: playlist) }
        }
        if !tracks.contains(where: \.isEpisode) {
            Button("Add to Liked Songs") { model.setSaved(tracks, true) }
            if source.context != nil, source.context == model.likedContext {
                Button("Remove from Liked Songs") { model.setSaved(tracks, false) }
            }
        }
        if let first, tracks.count == 1, !first.isEpisode {
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
