import SwiftUI

struct ContentView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        Group {
            switch model.connection {
            case .connecting:
                ProgressView("Connecting to Spotlite…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .failed(let message):
                ContentUnavailableView {
                    Label("Can't reach Spotlite", systemImage: "bolt.horizontal.circle")
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
                ToolbarItemGroup(placement: .navigation) {
                    Button { model.back() } label: { Image(systemName: "chevron.left") }
                        .disabled(!model.canGoBack)
                        .help("Back")
                    Button { model.forward() } label: { Image(systemName: "chevron.right") }
                        .disabled(!model.canGoForward)
                        .help("Forward")
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
        .searchable(text: $model.searchText, placement: .sidebar, prompt: "Search")
        .onChange(of: model.searchText) { _, text in
            if !text.trimmingCharacters(in: .whitespaces).isEmpty, model.root != .search {
                model.root = .search
            }
        }
    }

    private var title: String {
        switch model.route {
        case .home: "Home"
        case .search: "Search"
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
            Label("Home", systemImage: "house").tag(Route.home)
            Section("Library") {
                Label("Liked Songs", systemImage: "heart")
                    .tag(Route.liked)
                    .dropDestination(for: String.self) { uris, _ in
                        model.send("library.set", ["uris": uris, "saved": true], done: "Saved to Liked Songs")
                        return true
                    }
                Label("Albums", systemImage: "square.stack").tag(Route.albums)
                Label("Artists", systemImage: "music.microphone").tag(Route.artists)
                Label("Podcasts", systemImage: "dot.radiowaves.left.and.right").tag(Route.podcasts)
                Label("Episodes", systemImage: "bookmark").tag(Route.episodes)
                Label("Top Songs", systemImage: "chart.line.uptrend.xyaxis").tag(Route.topSongs)
            }
            Section {
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
                Label(row.name, systemImage: "folder")
            }
        } else if let playlist = row.playlist {
            Label(playlist.name, systemImage: "music.note.list")
                .lineLimit(1)
                .tag(Route.playlist(playlist.id))
                .contextMenu { PlaylistMenu(playlist: playlist) }
                .dropDestination(for: String.self) { uris, _ in
                    guard playlist.editable == true else { return false }
                    model.send("playlist.add", ["id": playlist.id, "uris": uris], done: "Added to \(playlist.name)")
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

    var body: some View {
        Form {
            TextField("Name", text: $name)
            TextField("Description", text: $description, axis: .vertical)
                .lineLimit(3...6)
            Toggle("Public", isOn: $isPublic)
        }
        .formStyle(.grouped)
        .frame(width: 420)
        .navigationTitle("Edit Details")
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
