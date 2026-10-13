import SwiftUI

struct ContentView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        Group {
            switch model.connection {
            case .connecting:
                ProgressView("Connecting to SwiftSpot…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .failed(let message):
                ContentUnavailableView {
                    Label("Can't reach SwiftSpot", systemImage: "bolt.horizontal.circle")
                } description: {
                    Text(message)
                } actions: {
                    Button("Try Again") { Task { await model.connect() } }
                }
            case .connected:
                if model.hello?.signedIn == true {
                    Shell()
                } else {
                    SignInView()
                }
            }
        }
        .sheet(item: Binding(
            get: { model.newPlaylistSongs.map(NewPlaylistRequest.init) },
            set: { model.newPlaylistSongs = $0?.songs })
        ) { request in
            NewPlaylistSheet(songs: request.songs)
        }
        .sheet(item: $model.editingPlaylist) { playlist in
            EditPlaylistSheet(playlist: playlist)
        }
        // No least size here: the split view sums its columns' own, and a
        // second one fights it whenever the inspector opens, until AppKit
        // gives up on the window's constraints.
        .frame(minHeight: 560)
    }
}

private struct NewPlaylistRequest: Identifiable {
    var songs: [Track]
    var id: String { songs.map(\.uri).joined(separator: ",") }
}

/// The signed-in window: the sidebar, the page, the panels and the player.
struct Shell: View {
    @Environment(AppModel.self) private var model
    @FocusState private var searchFocused: Bool

    var body: some View {
        @Bindable var model = model
        NavigationSplitView {
            Sidebar()
                .navigationSplitViewColumnWidth(min: 200, ideal: 230, max: 320)
        } detail: {
            // The page takes the width it is given and never asks for more:
            // a least width taken from content that changes as rows load
            // sends the split view round its constraints until AppKit gives
            // up. The reader has no least size of its own.
            GeometryReader { _ in
                ZStack(alignment: .bottom) {
                    RouteView(route: model.route)
                        .id(model.route)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    PlayerBar()
                        .padding(.horizontal, 20)
                        .padding(.bottom, 14)
                }
            }
            .frame(minWidth: 560, maxWidth: .infinity, maxHeight: .infinity)
            .overlay(alignment: .top) { Toast() }
            .navigationTitle(title)
            .toolbar {
                // As Music: Back once there is somewhere to go back to.
                ToolbarItemGroup(placement: .navigation) {
                    if model.canGoBack {
                        Button { model.back() } label: { Image(systemName: "chevron.left") }
                            .help("Back")
                    }
                    if model.canGoForward {
                        Button { model.forward() } label: { Image(systemName: "chevron.right") }
                            .help("Forward")
                    }
                }
                if model.route.sortable {
                    ToolbarItem(placement: .primaryAction) { SortMenu() }
                }
            }
            .inspector(isPresented: Binding(
                get: { model.panel != nil },
                set: { if !$0 { model.panel = nil } })
            ) {
                Group {
                    switch model.panel {
                    case .queue: QueueView()
                    case .lyrics: LyricsView()
                    case nil: EmptyView()
                    }
                }
                .inspectorColumnWidth(min: 260, ideal: 300, max: 420)
            }
        }
        .overlay {
            if model.showNowPlaying {
                NowPlayingView()
                    .transition(.opacity.combined(with: .scale(scale: 1.02)))
                    .ignoresSafeArea()
            }
        }
        .animation(.easeInOut(duration: 0.25), value: model.showNowPlaying)
        .toolbar(model.showNowPlaying ? .hidden : .automatic, for: .windowToolbar)
        // Music's field at the toolbar's end: Find on the library's pages,
        // which filters what they show, and Search everywhere else.
        .searchable(
            text: model.route.findable ? $model.findText : $model.searchText,
            placement: .toolbar,
            prompt: model.route.findable ? "Find in \(title)" : "Search")
        // Typing a search anywhere but Search goes there, with the words.
        .onChange(of: model.searchText) { _, text in
            if !text.trimmingCharacters(in: .whitespaces).isEmpty, model.route != .search {
                model.root = .search
            }
        }
        .searchFocused($searchFocused)
        .onChange(of: model.searchFocused) { _, wanted in
            guard wanted else { return }
            searchFocused = true
            model.searchFocused = false
        }
        // Space plays and pauses, as in Music; a text field being typed in
        // takes its own spaces first.
        .onKeyPress(.space) {
            model.toggle()
            return .handled
        }
    }

    private var title: String {
        switch model.route {
        case .home: "Home"
        case .search: "Search"
        case .recentlyAdded: "Recently Added"
        case .allPlaylists: "All Playlists"
        case .liked: "Liked Songs"
        case .albums: "Albums"
        case .artists: "Artists"
        case .podcasts: "Podcasts"
        case .episodes: "Your Episodes"
        case .topSongs: "Top Songs"
        case .playlist(let id): model.playlists.first { $0.id == id }?.name ?? "Playlist"
        case .album: "Album"
        case .artist: "Artist"
        case .show: "Podcast"
        case .user: "Profile"
        case .radio: "Radio"
        case .discography: "Discography"
        }
    }
}

// MARK: Sidebar

struct Sidebar: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        List(selection: Binding(
            get: { model.root },
            set: { if let route = $0 { model.root = route } })
        ) {
            SidebarLabel("Search", "magnifyingglass").tag(Route.search)
            SidebarLabel("Home", "house").tag(Route.home)
            Section("Library") {
                SidebarLabel("Recently Added", "clock").tag(Route.recentlyAdded)
                SidebarLabel("Artists", "music.microphone").tag(Route.artists)
                SidebarLabel("Albums", "square.stack").tag(Route.albums)
                // Liked Songs is Music's Songs: what this account keeps.
                SidebarLabel("Songs", "music.note")
                    .tag(Route.liked)
                    .dropDestination(for: String.self) { uris, _ in
                        model.send("library.set", ["uris": uris.map(songURI), "saved": true], done: "Added to Liked Songs")
                        return true
                    }
                SidebarLabel("Podcasts", "dot.radiowaves.left.and.right").tag(Route.podcasts)
                SidebarLabel("Episodes", "bookmark").tag(Route.episodes)
                SidebarLabel("Top Songs", "chart.line.uptrend.xyaxis").tag(Route.topSongs)
            }
            Section {
                SidebarLabel("All Playlists", "square.grid.3x3").tag(Route.allPlaylists)
                ForEach(model.sidebarRows) { row in
                    SidebarRowView(row: row)
                }
            } header: {
                HStack {
                    Text("Playlists")
                    Spacer()
                    Button { model.requestNewPlaylist() } label: { Image(systemName: "plus") }
                        .buttonStyle(.plain)
                        .help("New Playlist")
                        // In line with the rows' trailing edge, not the
                        // sidebar's.
                        .padding(.trailing, 10)
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            Account()
                .background(.bar)
        }
    }
}

struct SidebarRowView: View {
    @Environment(AppModel.self) private var model
    var row: SidebarRow

    var body: some View {
        if row.kind == "folder" {
            DisclosureGroup {
                ForEach(row.children ?? []) { child in SidebarRowView(row: child) }
            } label: {
                SidebarLabel(row.name, "folder")
            }
        } else if let playlist = row.playlist {
            SidebarLabel(playlist.name, "music.note.list")
                .lineLimit(1)
                .tag(Route.playlist(playlist.id))
                .contextMenu { PlaylistMenu(playlist: playlist) }
                .dropDestination(for: String.self) { uris, _ in
                    guard playlist.editable == true else { return false }
                    model.send("playlist.add", ["id": playlist.id, "uris": uris.map(songURI)], done: "Added to \(playlist.name)")
                    return true
                }
        }
    }
}

struct PlaylistMenu: View {
    @Environment(AppModel.self) private var model
    var playlist: Playlist

    var body: some View {
        Button("Play") { model.play(context: playlist.uri, shuffle: false) }
        Button("Shuffle") { model.play(context: playlist.uri, shuffle: true) }
        Divider()
        if playlist.owned == true {
            Button("Edit Details…") { model.editingPlaylist = playlist }
        }
        Button("Start Radio") { model.startRadio(seed: playlist.uri) }
        Button("Copy Link") { model.copyLink(playlist.uri) }
        Divider()
        Button(playlist.owned == true ? "Delete Playlist" : "Remove from Library", role: .destructive) {
            model.delete(playlist)
        }
    }
}

struct Account: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        if let user = model.hello?.user {
            Menu {
                if let product = user.product { Text("Spotify \(product.capitalized)") }
                Button("Profile") { model.open(.user(user.id)) }
                Button("Settings…") { openSettings() }
                Divider()
                Button("Sign Out") { model.signOut() }
            } label: {
                HStack(spacing: 8) {
                    Artwork(url: user.image, size: 24, placeholder: "person.fill", circle: true)
                    Text(user.name).font(.callout.weight(.medium)).lineLimit(1)
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .menuStyle(.button)
            .buttonStyle(.plain)
            .menuIndicator(.hidden)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
        }
    }
}

// MARK: Messages

struct Toast: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        if let message = model.toast {
            Text(message)
                .font(.callout.weight(.medium))
                .padding(.horizontal, 16)
                .padding(.vertical, 9)
                .glassEffect(.regular, in: .capsule)
                .padding(.top, 10)
                .transition(.move(edge: .top).combined(with: .opacity))
        }
    }
}

// MARK: Signing in

struct SignInView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let hello = model.hello
        VStack(spacing: 18) {
            Image(systemName: "music.note")
                .font(.system(size: 48, weight: .semibold))
                .foregroundStyle(.tint)
            Text("Sign in to Spotify").font(.largeTitle.weight(.bold))
            switch hello?.auth {
            case "waiting_for_browser":
                Text("Finish signing in in your browser.").foregroundStyle(.secondary)
                if let url = hello?.signInUrl.flatMap(URL.init(string:)) {
                    Link("Open the sign-in page again", destination: url)
                }
                Button("Cancel") { model.send("auth.cancel") }
            case "connecting", "starting":
                ProgressView("Connecting…")
            default:
                if let error = hello?.authError {
                    Text(error).foregroundStyle(.red).multilineTextAlignment(.center)
                }
                Button {
                    model.signIn()
                } label: {
                    Text("Sign In with Spotify").frame(minWidth: 200)
                }
                .buttonStyle(.glassProminent)
                .controlSize(.extraLarge)
            }
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task {
            // The auth event says when it is done; asking keeps the page
            // current through the browser round trip too.
            while !Task.isCancelled, model.hello?.signedIn != true {
                if let hello = try? await model.call("hello", as: Hello.self) {
                    model.hello = hello
                    if hello.signedIn { await model.refreshAll() }
                }
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }
}

// MARK: Playlist sheets

struct NewPlaylistSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    var songs: [Track]
    @State private var name = ""
    @State private var isPublic = false

    var body: some View {
        Form {
            TextField("Name", text: $name, prompt: Text("My Playlist"))
            Toggle("Public", isOn: $isPublic)
            if !songs.isEmpty {
                Text(songs.count == 1 ? "Adds “\(songs[0].name)”." : "Adds \(songs.count) songs.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 380)
        .navigationTitle("New Playlist")
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button("Create") {
                    let title = name.trimmingCharacters(in: .whitespaces)
                    model.createPlaylist(named: title.isEmpty ? "My Playlist" : title, isPublic: isPublic, with: songs)
                    dismiss()
                }
            }
        }
    }
}

struct EditPlaylistSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    var playlist: Playlist
    @State private var name = ""
    @State private var description = ""
    @State private var isPublic = false
    @State private var choosing = false

    private func chooseCover() { choosing = true }

    var body: some View {
        Form {
            HStack(spacing: 14) {
                Artwork(url: playlist.cover ?? playlist.image, size: 72, radius: 6)
                Button("Choose Cover Image…") { chooseCover() }
            }
            TextField("Name", text: $name)
            TextField("Description", text: $description, axis: .vertical)
                .lineLimit(3...6)
            Toggle("Public", isOn: $isPublic)
        }
        .formStyle(.grouped)
        .frame(width: 420)
        .navigationTitle("Edit Details")
        .fileImporter(isPresented: $choosing, allowedContentTypes: [.image]) { result in
            guard case .success(let url) = result else { return }
            let access = url.startAccessingSecurityScopedResource()
            defer { if access { url.stopAccessingSecurityScopedResource() } }
            // The engine reads the file; a copy it can always reach.
            let copy = FileManager.default.temporaryDirectory
                .appendingPathComponent("swiftspot-cover-\(UUID().uuidString).\(url.pathExtension)")
            try? FileManager.default.copyItem(at: url, to: copy)
            model.send("playlist.cover", ["id": playlist.id, "path": copy.path], done: "Uploading the cover")
        }
        .onAppear {
            name = playlist.name
            description = playlist.description ?? ""
            isPublic = playlist.isPublic ?? false
        }
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button("Save") {
                    model.send("playlist.edit", [
                        "id": playlist.id, "name": name, "description": description, "public": isPublic,
                    ])
                    dismiss()
                }
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
    }
}

/// The song in a drag's payload: a row from an editable playlist carries
/// its playlist and place after a `#`.
func songURI(_ payload: String) -> String {
    String(payload.split(separator: "#").first ?? Substring(payload))
}

/// A sidebar row as Music draws it: the symbol in the accent colour.
struct SidebarLabel: View {
    var title: String
    var symbol: String

    init(_ title: String, _ symbol: String) {
        self.title = title
        self.symbol = symbol
    }

    var body: some View {
        Label {
            Text(title)
        } icon: {
            Image(systemName: symbol).foregroundStyle(.tint)
        }
    }
}

/// Music's sort button beside Find, for a page of songs.
struct SortMenu: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Menu {
            Picker("Sort By", selection: Binding(
                get: { model.listSort?.column },
                set: { column in
                    model.listSort = column.map { SongSort(column: $0, ascending: model.listSort?.ascending ?? true) }
                })
            ) {
                Text(model.route == .liked ? "Date Added" : "Playlist Order").tag(SongSort.Column?.none)
                Text("Title").tag(SongSort.Column?.some(.title))
                Text("Artist").tag(SongSort.Column?.some(.artist))
                Text("Album").tag(SongSort.Column?.some(.album))
                Text("Date Added").tag(SongSort.Column?.some(.added))
                Text("Time").tag(SongSort.Column?.some(.duration))
            }
            .pickerStyle(.inline)
            if model.listSort != nil {
                Divider()
                Picker("Order", selection: Binding(
                    get: { model.listSort?.ascending ?? true },
                    set: { model.listSort?.ascending = $0 })
                ) {
                    Text("Ascending").tag(true)
                    Text("Descending").tag(false)
                }
                .pickerStyle(.inline)
            }
        } label: {
            Image(systemName: "line.3.horizontal.decrease")
        }
        .menuIndicator(.hidden)
        .help("Sort")
    }
}
