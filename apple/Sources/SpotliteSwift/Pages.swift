import SwiftUI

/// The page a route shows.
struct RouteView: View {
    var route: Route

    var body: some View {
        switch route {
        case .home: HomeView()
        case .search: SearchView()
        case .liked: SongsView(method: "library.liked", params: [:])
        case .topSongs: SongsView(method: "top_songs.get", params: [:])
        case .playlist(let id): SongsView(method: "playlist.get", params: ["id": id])
        case .album(let id): SongsView(method: "album.get", params: ["id": id])
        case .radio(let seed): SongsView(method: "radio.get", params: ["seed": seed])
        case .artist(let id): ArtistView(id: id)
        case .show(let id): ShowView(id: id)
        case .user(let id): UserView(id: id)
        case .albums: LibraryGrid(method: "library.albums", title: "Albums")
        case .artists: LibraryGrid(method: "library.artists", title: "Artists")
        case .podcasts: LibraryGrid(method: "library.shows", title: "Podcasts")
        case .episodes: SongsView(method: "library.episodes", params: [:])
        }
    }
}

// MARK: Home

struct HomeView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Loader(method: "home.get", loading: { $0.loading }) { (home: HomePage, _) in
            PageScroll {
                PageTitle(text: "Home")
                VStack(alignment: .leading, spacing: 34) {
                    ForEach(home.shelves) { shelf in
                        if let cards = shelf.cards, !cards.isEmpty {
                            ShelfView(title: shelf.title, cards: cards)
                        } else if let tracks = shelf.tracks, !tracks.isEmpty, tracks.allSatisfy(\.isEpisode) {
                            EpisodeShelf(title: shelf.title, episodes: tracks)
                        } else if let tracks = shelf.tracks, !tracks.isEmpty {
                            VStack(alignment: .leading, spacing: 10) {
                                HStack(alignment: .firstTextBaseline) {
                                    Text(shelf.title).font(.title2.weight(.bold))
                                    Spacer()
                                    if let page = shelf.seeAll, let route = Route(page: page) {
                                        Button("See All") { model.open(route) }.buttonStyle(.link)
                                    }
                                }
                                SongList(
                                    tracks: tracks.enumerated().map { var track = $1; track.position = $0; return track },
                                    columns: [.number, .cover, .album])
                            }
                        }
                    }
                }
                LoadingFooter(loading: home.loading, error: nil)
            }
        }
    }
}

// MARK: Search

struct SearchView: View {
    @Environment(AppModel.self) private var model
    @State private var results: SearchResults?
    @State private var scope = "All"
    @State private var history: [String] = []

    private let scopes = ["All", "Songs", "Artists", "Albums", "Playlists", "Podcasts"]

    var body: some View {
        let query = model.searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        PageScroll {
            if query.isEmpty {
                if history.isEmpty {
                    ContentUnavailableView(
                        "Search Spotify", systemImage: "magnifyingglass",
                        description: Text("Find songs, artists, albums, playlists and podcasts."))
                        .frame(maxWidth: .infinity, minHeight: 400)
                } else {
                    PageTitle(text: "Recent Searches")
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(history, id: \.self) { past in
                            HStack {
                                Image(systemName: "clock.arrow.circlepath").foregroundStyle(.secondary)
                                Button(past) { model.searchText = past }
                                    .buttonStyle(.plain)
                                Spacer()
                                Button {
                                    history.removeAll { $0 == past }
                                    model.send("search.forget", ["query": past])
                                } label: { Image(systemName: "xmark") }
                                    .buttonStyle(.plain)
                                    .foregroundStyle(.secondary)
                                    .help("Remove")
                            }
                            .padding(.vertical, 9)
                            Divider()
                        }
                    }
                    .frame(maxWidth: 520)
                }
            } else {
                Picker("Show", selection: $scope) {
                    ForEach(scopes, id: \.self) { Text($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(maxWidth: 520)
                .padding(.bottom, 22)
                if let results, results.query == query {
                    resultsView(results)
                    LoadingFooter(loading: results.loading, error: results.error)
                } else {
                    ProgressView().frame(maxWidth: .infinity).padding(40)
                }
            }
        }
        .task(id: query) {
            if query.isEmpty {
                struct History: Decodable { var queries: [String] }
                history = (try? await model.call("search.history", as: History.self))?.queries ?? []
            } else {
                await search(query)
            }
        }
    }

    @ViewBuilder
    private func resultsView(_ results: SearchResults) -> some View {
        VStack(alignment: .leading, spacing: 30) {
            if scope == "All" || scope == "Songs", !results.tracks.isEmpty {
                section("Songs") {
                    SongList(tracks: Array(results.tracks.prefix(scope == "All" ? 8 : 50)),
                             columns: [.cover, .album])
                }
            }
            if scope == "All" || scope == "Artists", !results.artists.isEmpty {
                cards("Artists", results.artists)
            }
            if scope == "All" || scope == "Albums", !results.albums.isEmpty {
                cards("Albums", results.albums)
            }
            if scope == "All" || scope == "Playlists", !results.playlists.isEmpty {
                cards("Playlists", results.playlists)
            }
            if scope == "All" || scope == "Podcasts" {
                if !results.shows.isEmpty { cards("Podcasts", results.shows) }
                if !results.episodes.isEmpty {
                    section("Episodes") {
                        SongList(tracks: Array(results.episodes.prefix(scope == "All" ? 5 : 50)),
                                 columns: [.cover, .date])
                    }
                }
            }
            if !results.loading && results.tracks.isEmpty && results.artists.isEmpty
                && results.albums.isEmpty && results.playlists.isEmpty && results.shows.isEmpty {
                ContentUnavailableView.search(text: results.query)
            }
        }
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.title2.weight(.bold))
            content()
        }
    }

    @ViewBuilder
    private func cards(_ title: String, _ cards: [Card]) -> some View {
        if scope == "All" {
            ShelfView(title: title, cards: cards)
        } else {
            section(title) { CardGrid(cards: cards) }
        }
    }

    private func search(_ query: String) async {
        guard !query.isEmpty else { return }
        // Typing settles before Spotify is asked.
        try? await Task.sleep(for: .milliseconds(300))
        guard !Task.isCancelled else { return }
        for attempt in 0..<40 {
            guard let next = try? await model.call("search", ["query": query], as: SearchResults.self),
                  !Task.isCancelled else { return }
            results = numbered(next)
            if !next.loading { return }
            try? await Task.sleep(for: .milliseconds(attempt < 10 ? 200 : 600))
        }
    }

    private func numbered(_ results: SearchResults) -> SearchResults {
        var results = results
        results.tracks = results.tracks.enumerated().map { var track = $1; track.position = $0; return track }
        results.episodes = results.episodes.enumerated().map { var track = $1; track.position = $0; return track }
        return results
    }
}

// MARK: Songs

/// A playlist, an album, Liked Songs, a radio or Top Songs.
struct SongsView: View {
    @Environment(AppModel.self) private var model
    var method: String
    var params: [String: String]

    @State private var filter = ""
    @State private var sort: SongSort?

    var body: some View {
        Loader(method: method, params: params, loading: { $0.loading }) {
            (page: SongsPage, more: @escaping () -> Void) in
            let tracks = numbered(page.tracks ?? [])
            let shown = sort.map { $0.apply(filtered(tracks)) } ?? filtered(tracks)
            let playlistID = params["id"].flatMap { method == "playlist.get" ? $0 : nil }
            PageScroll {
                PageHeader(
                    cover: page.cover,
                    title: page.title ?? "",
                    owner: page.owner,
                    ownerRoute: page.ownerRoute.flatMap(Route.init(page:)),
                    facts: page.facts ?? [],
                    description: page.description,
                    glyph: method == "library.liked" ? "heart.fill"
                        : method == "library.episodes" ? "bookmark.fill" : nil
                ) {
                    if page.context != nil {
                        PlayButtons(context: page.context, uris: page.stationUris)
                    }
                    if let uri = page.uri, page.owned != true, method != "library.liked" {
                        Button {
                            model.toggleLibrary(uri: uri)
                        } label: {
                            Image(systemName: page.saved == true ? "checkmark" : "plus")
                                .frame(width: 20)
                        }
                        .help(page.saved == true ? "Remove from Library" : "Add to Library")
                    }
                    if let uri = page.uri {
                        Menu {
                            if let playlist = model.playlists.first(where: { $0.id == playlistID }), page.owned == true {
                                Button("Edit Details…") { model.editingPlaylist = playlist }
                                Button("Delete Playlist", role: .destructive) { model.delete(playlist) }
                                Divider()
                            }
                            Button("Add All to Queue") { model.addToQueue(tracks) }
                            Button("Start Radio") { model.startRadio(seed: uri) }
                            Button("Copy Link") { model.copyLink(uri) }
                        } label: {
                            Image(systemName: "ellipsis").frame(width: 20)
                        }
                        .menuIndicator(.hidden)
                        .menuStyle(.borderlessButton)
                        .glassMenu()
                    }
                }
                if tracks.count > 12 {
                    TextField("Find in \(page.kind ?? "List")", text: $filter)
                        .textFieldStyle(.roundedBorder)
                        .frame(maxWidth: 260)
                        .padding(.bottom, 12)
                }
                if method == "library.episodes" {
                    VStack(spacing: 0) {
                        ForEach(shown) { episode in
                            EpisodeRow(episode: episode)
                                .onAppear {
                                    if episode.position == tracks.count - 1, page.more == true { more() }
                                }
                            Divider()
                        }
                    }
                } else {
                    SongList(
                        tracks: shown,
                        columns: columns(for: page),
                        source: SongSource(
                            context: page.context,
                            editablePlaylist: page.editable == true ? playlistID : nil,
                            reordered: sort != nil || !filter.isEmpty),
                        more: page.more == true && filter.isEmpty && sort == nil ? more : nil,
                        sort: method == "album.get" ? nil : $sort)
                }
                LoadingFooter(loading: page.loading, error: page.error)
                if !page.loading, tracks.isEmpty, page.error == nil {
                    ContentUnavailableView("Nothing here yet", systemImage: "music.note.list")
                        .padding(.top, 40)
                }
            }
        }
    }

    private func columns(for page: SongsPage) -> SongColumns {
        if method == "album.get" { return .albumTracks }
        var columns: SongColumns = [.number, .cover]
        if page.showAlbum != false { columns.insert(.album) }
        if page.showAdded == true { columns.insert(.added) }
        if page.tracks?.contains(where: { $0.addedBy != nil }) == true { columns.insert(.addedBy) }
        return columns
    }

    private func numbered(_ tracks: [Track]) -> [Track] {
        tracks.enumerated().map { var track = $1; track.position = $0; return track }
    }

    private func filtered(_ tracks: [Track]) -> [Track] {
        let needle = filter.trimmingCharacters(in: .whitespaces).lowercased()
        guard !needle.isEmpty else { return tracks }
        return tracks.filter {
            $0.name.lowercased().contains(needle)
                || $0.artistNames.lowercased().contains(needle)
                || $0.albumName.lowercased().contains(needle)
        }
    }
}

// MARK: Artist

struct ArtistView: View {
    @Environment(AppModel.self) private var model
    var id: String
    @State private var showAllTop = false

    var body: some View {
        Loader(method: "artist.get", params: ["id": id], loading: { $0.loading }) {
            (page: ArtistPage, more: @escaping () -> Void) in
            let top = (page.topTracks ?? []).enumerated().map { var track = $1; track.position = $0; return track }
            PageScroll {
                if let artist = page.artist {
                    PageHeader(
                        cover: artist.cover,
                        title: artist.name,
                        facts: [artist.followers.map { "\($0.formatted()) followers" }].compactMap { $0 }
                            + (artist.genres ?? []).prefix(2),
                        round: true
                    ) {
                        PlayButtons(context: artist.uri)
                        Button(page.following == true ? "Following" : "Follow") {
                            model.follow(artist: id, follow: page.following != true)
                        }
                        Menu {
                            Button("Start Radio") { model.startRadio(seed: artist.uri) }
                            Button("Copy Link") { model.copyLink(artist.uri) }
                        } label: {
                            Image(systemName: "ellipsis").frame(width: 20)
                        }
                        .menuIndicator(.hidden)
                        .menuStyle(.borderlessButton)
                        .glassMenu()
                    }
                }
                VStack(alignment: .leading, spacing: 32) {
                    if !top.isEmpty {
                        VStack(alignment: .leading, spacing: 10) {
                            HStack {
                                Text("Top Songs").font(.title2.weight(.bold))
                                Spacer()
                                if top.count > 5 {
                                    Button(showAllTop ? "Show Less" : "Show All") { showAllTop.toggle() }
                                        .buttonStyle(.link)
                                }
                            }
                            SongList(tracks: showAllTop ? top : Array(top.prefix(5)), columns: [.number, .cover, .album])
                        }
                    }
                    // The discography comes in pages; the end of a shelf asks
                    // for the next.
                    let next = page.more == true ? more : nil
                    if let albums = page.albums, !albums.isEmpty {
                        ShelfView(title: "Albums", cards: albums, more: next)
                    }
                    if let singles = page.singles, !singles.isEmpty {
                        ShelfView(title: "Singles & EPs", cards: singles, more: next)
                    }
                    if let compilations = page.compilations, !compilations.isEmpty {
                        ShelfView(title: "Compilations", cards: compilations, more: next)
                    }
                    if let appears = page.appearsOn, !appears.isEmpty { ShelfView(title: "Appears On", cards: appears) }
                    if let related = page.related, !related.isEmpty { ShelfView(title: "Similar Artists", cards: related) }
                }
                LoadingFooter(loading: page.loading, error: page.error)
            }
        }
    }
}

// MARK: Podcast

struct ShowView: View {
    @Environment(AppModel.self) private var model
    var id: String

    var body: some View {
        Loader(method: "show.get", params: ["id": id], loading: { $0.loading }) {
            (page: ShowPage, more: @escaping () -> Void) in
            let episodes = (page.episodes ?? []).enumerated().map { var track = $1; track.position = $0; return track }
            PageScroll {
                if let show = page.show {
                    PageHeader(
                        cover: show.cover,
                        title: show.name,
                        owner: show.publisher,
                        facts: ["Podcast", show.total.map { "\($0) episodes" }].compactMap { $0 },
                        description: show.description
                    ) {
                        Button {
                            if let first = episodes.first { model.play(first, in: nil, tracks: [first]) }
                        } label: {
                            Label("Latest Episode", systemImage: "play.fill")
                        }
                        .disabled(episodes.isEmpty)
                        Button {
                            model.toggleLibrary(uri: show.uri)
                        } label: {
                            Label(page.saved == true ? "Following" : "Follow",
                                  systemImage: page.saved == true ? "checkmark" : "plus")
                        }
                    }
                }
                VStack(spacing: 0) {
                    ForEach(episodes) { episode in
                        EpisodeRow(episode: episode)
                            .onAppear { if episode.position == episodes.count - 1, page.more == true { more() } }
                        Divider()
                    }
                }
                LoadingFooter(loading: page.loading, error: page.error)
            }
        }
    }
}

struct EpisodeRow: View {
    @Environment(AppModel.self) private var model
    var episode: Track

    var body: some View {
        let playing = model.player.uri == episode.uri
        HStack(alignment: .top, spacing: 14) {
            Artwork(url: episode.image, size: 64, radius: 6)
            VStack(alignment: .leading, spacing: 4) {
                Text(shortDate(episode.releaseDate).uppercased())
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
                Text(episode.name)
                    .font(.headline)
                    .lineLimit(2)
                if let description = episode.description {
                    Text(description)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                HStack(spacing: 10) {
                    Button {
                        if playing { model.toggle() } else { model.play(episode, in: nil, tracks: [episode]) }
                    } label: {
                        Label(playing && model.player.playing == true ? "Pause" : "Play",
                              systemImage: playing && model.player.playing == true ? "pause.fill" : "play.fill")
                    }
                    .buttonStyle(.glass)
                    .controlSize(.small)
                    Text(remaining)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    if episode.played == true {
                        Image(systemName: "checkmark.circle.fill").foregroundStyle(.tint)
                    }
                }
                .padding(.top, 2)
            }
            Spacer()
        }
        .padding(.vertical, 12)
        .contextMenu { SongMenu(tracks: [episode], source: SongSource()) }
    }

    private var remaining: String {
        if let resume = episode.resumeMs, resume > 0, episode.played != true {
            return "\(formatTime(ms: episode.durationMs - resume)) left"
        }
        return formatTime(ms: episode.durationMs)
    }
}

// MARK: Profile

struct UserView: View {
    var id: String

    var body: some View {
        Loader(method: "user.get", params: ["id": id], loading: { $0.loading }) {
            (page: UserPage, _) in
            PageScroll {
                if let user = page.user {
                    PageHeader(
                        cover: user.image,
                        title: user.name,
                        facts: ["Profile", user.followers.map { "\($0.formatted()) followers" }].compactMap { $0 },
                        round: true
                    ) { EmptyView() }
                }
                if let playlists = page.playlists, !playlists.isEmpty {
                    Text("Public Playlists").font(.title2.weight(.bold)).padding(.bottom, 12)
                    CardGrid(cards: playlists)
                }
                LoadingFooter(loading: page.loading, error: page.error)
            }
        }
    }
}

// MARK: Library

struct LibraryGrid: View {
    var method: String
    var title: String

    var body: some View {
        Loader(method: method, loading: { $0.loading }) {
            (page: CardsPage, more: @escaping () -> Void) in
            PageScroll {
                PageTitle(text: title)
                CardGrid(cards: page.cards, more: page.more == true ? more : nil)
                LoadingFooter(loading: page.loading, error: page.error)
                if !page.loading, page.cards.isEmpty {
                    ContentUnavailableView("Nothing here yet", systemImage: "square.stack")
                        .padding(.top, 40)
                }
            }
        }
    }
}

/// Episodes to listen to, as cards that play where they left off.
struct EpisodeShelf: View {
    @Environment(AppModel.self) private var model
    var title: String
    var episodes: [Track]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.title2.weight(.bold))
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: 20) {
                    ForEach(episodes) { episode in
                        VStack(alignment: .leading, spacing: 6) {
                            ZStack(alignment: .bottomLeading) {
                                Artwork(url: episode.image, size: 168, radius: 8)
                                    .shadow(color: .black.opacity(0.15), radius: 6, y: 4)
                                if let resume = episode.resumeMs, resume > 0, episode.durationMs > 0 {
                                    Capsule().fill(.white.opacity(0.35))
                                        .frame(width: 148, height: 4)
                                        .overlay(alignment: .leading) {
                                            Capsule().fill(.white)
                                                .frame(width: 148 * min(Double(resume) / Double(episode.durationMs), 1), height: 4)
                                        }
                                        .padding(10)
                                }
                            }
                            Text(episode.name)
                                .font(.callout.weight(.medium))
                                .lineLimit(2)
                            Text(episode.subtitle ?? "")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                        .frame(width: 168, alignment: .leading)
                        .contentShape(Rectangle())
                        .onTapGesture { model.play(episode, in: nil, tracks: [episode]) }
                        .contextMenu {
                            Button("Play") { model.play(episode, in: nil, tracks: [episode]) }
                            if let show = episode.showId {
                                Button("Go to Podcast") { model.open(.show(show)) }
                            }
                        }
                    }
                }
            }
            .scrollIndicators(.never)
        }
    }
}
