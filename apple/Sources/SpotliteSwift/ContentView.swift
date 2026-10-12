import SwiftUI

struct ContentView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        NavigationSplitView {
            Sidebar()
                .navigationSplitViewColumnWidth(min: 200, ideal: 240, max: 320)
        } detail: {
            ZStack(alignment: .bottom) {
                if let message = model.connection {
                    ContentUnavailableView(
                        "Can't reach Spotlite", systemImage: "bolt.horizontal.circle",
                        description: Text(message))
                } else if let detail = model.detail {
                    DetailView(detail: detail)
                } else {
                    ProgressView()
                }
                PlayerBar()
                    .padding(.horizontal, 24)
                    .padding(.bottom, 14)
            }
        }
    }
}

struct Sidebar: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        List(selection: $model.selection) {
            Section("Library") {
                Label("Liked Songs", systemImage: "heart")
                    .tag(SidebarItem.liked)
            }
            Section("Playlists") {
                ForEach(model.playlists) { playlist in
                    Label(playlist.name, systemImage: "music.note.list")
                        .lineLimit(1)
                        .tag(SidebarItem.playlist(playlist.id))
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            if let user = model.hello?.user {
                HStack(spacing: 8) {
                    Artwork(url: user.image, size: 24, radius: 12)
                    Text(user.name).font(.callout.weight(.medium))
                    Spacer()
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
            }
        }
    }
}
