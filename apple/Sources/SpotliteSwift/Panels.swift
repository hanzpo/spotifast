import SwiftUI

// MARK: Queue

/// What plays next: the songs queued by hand, then the context's.
struct QueueView: View {
    @Environment(AppModel.self) private var model
    @State private var tab = "next"

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(title: tab == "next" ? "Playing Next" : "History")
            Picker("", selection: $tab) {
                Text("Playing Next").tag("next")
                Text("History").tag("history")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, 16)
            .padding(.bottom, 8)
            if tab == "next" {
                upNext
            } else {
                HistoryList()
            }
        }
    }

    private var upNext: some View {
        let queue = model.queue
        return List {
            if let current = queue?.current {
                Section("Now Playing") {
                    QueueRow(track: current, playing: true)
                }
            }
            if let queued = queue?.queued, !queued.isEmpty {
                Section {
                    ForEach(Array(queued.enumerated()), id: \.offset) { index, track in
                        QueueRow(track: track, playing: false)
                            .onTapGesture(count: 2) { model.playQueued(at: index, queued: true) }
                            .contextMenu {
                                if queue?.reorderable == true {
                                    Button("Remove from Queue", systemImage: "minus.circle") {
                                        model.removeQueued(at: index)
                                    }
                                    Divider()
                                }
                                SongMenu(tracks: [track], source: SongSource())
                            }
                    }
                    .onMove { from, to in
                        guard queue?.reorderable == true, let start = from.first else { return }
                        model.moveQueued(from: start, to: to)
                    }
                    .onDelete { rows in
                        guard queue?.reorderable == true else { return }
                        // From the bottom up, so each index still names its row.
                        for row in rows.sorted(by: >) { model.removeQueued(at: row) }
                    }
                } header: {
                    HStack {
                        Text("Playing Next")
                        Spacer()
                        if queue?.clearable == true {
                            Button("Clear") { model.clearQueue() }
                                .buttonStyle(.link)
                        }
                    }
                }
            }
            if let next = queue?.next, !next.isEmpty {
                Section(queue?.context.map { "Up Next from \($0)" } ?? "Up Next") {
                    ForEach(Array(next.enumerated()), id: \.offset) { index, track in
                        QueueRow(track: track, playing: false)
                            .onTapGesture(count: 2) { model.playQueued(at: index, queued: false) }
                            .contextMenu { SongMenu(tracks: [track], source: SongSource()) }
                    }
                }
            }
            if queue == nil || queue?.loading == true {
                ProgressView().frame(maxWidth: .infinity)
            } else if queue?.current == nil, queue?.queued.isEmpty == true, queue?.next.isEmpty == true {
                ContentUnavailableView("Nothing queued", systemImage: "list.bullet")
            }
        }
        .listStyle(.inset)
        .safeAreaInset(edge: .bottom) {
            if queue?.current != nil {
                Button("Save as Playlist") {
                    model.send("queue.save", done: "Saving the queue as a playlist")
                }
                .buttonStyle(.glass)
                .padding(12)
            }
        }
        .task { await model.loadQueue() }
    }
}

/// What was played lately, newest first; a row plays again on a double
/// click.
struct HistoryList: View {
    @Environment(AppModel.self) private var model
    @State private var plays: RecentPlays?

    var body: some View {
        List {
            ForEach(Array((plays?.tracks ?? []).enumerated()), id: \.offset) { index, track in
                QueueRow(track: track, playing: false)
                    .onTapGesture(count: 2) { model.play(track, in: nil, tracks: [track]) }
                    .contextMenu { SongMenu(tracks: [track], source: SongSource()) }
                    .onAppear {
                        if index == (plays?.tracks.count ?? 0) - 1, plays?.more == true {
                            model.send("library.recent_more")
                            Task { await loadMore(after: plays?.tracks.count ?? 0) }
                        }
                    }
            }
            if plays == nil || plays?.loading == true {
                ProgressView().frame(maxWidth: .infinity)
            } else if plays?.tracks.isEmpty == true {
                ContentUnavailableView("Nothing played yet", systemImage: "clock")
            }
        }
        .listStyle(.inset)
        .task { await load() }
    }

    private func load() async {
        for attempt in 0..<600 {
            guard let next = try? await model.call("library.recent", as: RecentPlays.self) else { return }
            plays = next
            if !next.loading { return }
            try? await Task.sleep(for: pollDelay(attempt))
            if Task.isCancelled { return }
        }
    }

    /// Waits for the next page of plays: the engine says it is loading only
    /// while it has none, so wait for more rows, or for there to be no more.
    private func loadMore(after count: Int) async {
        for attempt in 0..<60 {
            try? await Task.sleep(for: pollDelay(attempt))
            if Task.isCancelled { return }
            guard let next = try? await model.call("library.recent", as: RecentPlays.self) else { return }
            plays = next
            if next.tracks.count > count || next.more != true { return }
        }
    }
}

struct QueueRow: View {
    var track: Track
    var playing: Bool

    var body: some View {
        HStack(spacing: 10) {
            Artwork(url: track.image, size: 36)
            VStack(alignment: .leading, spacing: 1) {
                Text(track.name)
                    .fontWeight(playing ? .semibold : .regular)
                    .foregroundStyle(playing ? AnyShapeStyle(.tint) : AnyShapeStyle(.primary))
                    .lineLimit(1)
                Text(track.artistNames)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
            Text(track.duration)
                .font(.callout.monospacedDigit())
                .foregroundStyle(.secondary)
        }
        .contentShape(Rectangle())
    }
}

struct PanelHeader: View {
    @Environment(AppModel.self) private var model
    var title: String

    var body: some View {
        HStack {
            Text(title).font(.title3.weight(.bold))
            Spacer()
            Button { model.panel = nil } label: { Image(systemName: "xmark") }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .help("Close")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}

// MARK: Lyrics

/// The words, following the song when they carry timings; a line seeks
/// there when clicked.
struct LyricsView: View {
    @Environment(AppModel.self) private var model
    @State private var following = true
    /// In the panel the view has its own heading; the Now Playing view
    /// shows the words alone, larger.
    var header = true
    var size: CGFloat = 22

    var body: some View {
        let lyrics = model.lyrics
        VStack(spacing: 0) {
            if header { PanelHeader(title: "Lyrics") }
            if let lyrics, !lyrics.lines.isEmpty, lyrics.uri == model.player.uri {
                TimelineView(.periodic(from: .now, by: 0.25)) { context in
                    let position = model.position(at: context.date)
                    let active = lyrics.synced ? activeLine(lyrics.lines, at: position) : nil
                    ScrollViewReader { proxy in
                        ScrollView {
                            VStack(alignment: .leading, spacing: 18) {
                                ForEach(Array(lyrics.lines.enumerated()), id: \.offset) { index, line in
                                    Text(line.text.isEmpty ? "♪" : line.text)
                                        .font(.system(size: size, weight: .bold))
                                        .foregroundStyle(lineStyle(index: index, active: active, synced: lyrics.synced))
                                        .multilineTextAlignment(.leading)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .id(index)
                                        .onTapGesture {
                                            if let time = line.timeMs {
                                                model.seek(ms: time)
                                                following = true
                                            }
                                        }
                                }
                            }
                            .padding(.horizontal, 20)
                            .padding(.vertical, 40)
                        }
                        .onScrollPhaseChange { _, phase in
                            if phase == .interacting { following = false }
                        }
                        .onChange(of: active) { _, line in
                            guard following, let line else { return }
                            withAnimation(.easeInOut(duration: 0.35)) {
                                proxy.scrollTo(line, anchor: UnitPoint(x: 0.5, y: 0.35))
                            }
                        }
                        .overlay(alignment: .bottom) {
                            if !following, lyrics.synced {
                                Button("Follow Along") { following = true }
                                    .buttonStyle(.glass)
                                    .padding()
                            }
                        }
                    }
                }
            } else if lyrics?.loading == true || (lyrics?.uri != model.player.uri && model.player.hasSong) {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if lyrics?.instrumental == true {
                ContentUnavailableView("Instrumental", systemImage: "pianokeys")
            } else {
                ContentUnavailableView(
                    model.player.hasSong ? "No lyrics for this song" : "Nothing playing",
                    systemImage: "quote.bubble")
            }
        }
        .task(id: model.player.uri) { await model.loadLyrics() }
    }

    private func activeLine(_ lines: [LyricsLine], at position: Int) -> Int? {
        lines.lastIndex { ($0.timeMs ?? Int.max) <= position }
    }

    private func lineStyle(index: Int, active: Int?, synced: Bool) -> AnyShapeStyle {
        guard synced, let active else { return AnyShapeStyle(.primary) }
        return index == active ? AnyShapeStyle(.primary) : AnyShapeStyle(.tertiary)
    }
}

// MARK: Devices

/// Spotify Connect: where playback goes.
struct DevicesView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("Play On").font(.headline)
                Spacer()
                Button { Task { await model.loadDevices() } } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.plain)
                    .help("Refresh")
            }
            .padding(.bottom, 6)
            if model.devices.isEmpty {
                Text("Looking for devices…")
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 8)
            }
            ForEach(model.devices) { device in
                Button {
                    model.transfer(to: device)
                } label: {
                    HStack(spacing: 10) {
                        Image(systemName: device.symbol)
                            .frame(width: 22)
                            .foregroundStyle(device.active ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                        VStack(alignment: .leading, spacing: 1) {
                            Text(device.name).lineLimit(1)
                            if device.local == true {
                                Text("This Mac").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                        Spacer()
                        if device.active { Image(systemName: "checkmark").foregroundStyle(.tint) }
                    }
                    .padding(.vertical, 6)
                    .padding(.horizontal, 8)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .background(RoundedRectangle(cornerRadius: 6).fill(device.active ? AnyShapeStyle(.quinary) : AnyShapeStyle(.clear)))
            }
        }
        .padding(14)
        .frame(width: 290)
        .task { await model.loadDevices() }
    }
}

// MARK: Now Playing

/// The full-window player: the cover large on a blur of itself, the song,
/// its controls, and its words beside it.
struct NowPlayingView: View {
    @Environment(AppModel.self) private var model
    @State private var lyrics = true

    var body: some View {
        let player = model.player
        GeometryReader { geometry in
            let art = min(geometry.size.height * 0.52, geometry.size.width * 0.36, 520)
            ZStack {
                // Opaque beneath the blur, which thins at its edges.
                Color.black
                Artwork(url: player.cover ?? player.image, size: max(geometry.size.width, geometry.size.height))
                    .scaleEffect(1.4)
                    .blur(radius: 80)
                    .overlay(Color.black.opacity(0.45))
                    .frame(width: geometry.size.width, height: geometry.size.height)
                    .clipped()
                HStack(spacing: 56) {
                    VStack(spacing: 22) {
                        Artwork(url: player.cover ?? player.image, size: art, radius: 12)
                            .shadow(color: .black.opacity(0.4), radius: 30, y: 16)
                        VStack(spacing: 4) {
                            Text(player.title ?? "")
                                .font(.title2.weight(.bold))
                                .lineLimit(1)
                            Text(player.subtitle ?? "")
                                .font(.title3)
                                .foregroundStyle(.white.opacity(0.7))
                                .lineLimit(1)
                        }
                        .frame(width: art)
                        TimelineView(.periodic(from: .now, by: 0.5)) { context in
                            let duration = max(player.durationMs ?? 0, 1)
                            let position = model.scrubbing.map { Int($0 * Double(duration)) }
                                ?? model.position(at: context.date)
                            VStack(spacing: 4) {
                                SeekLine(fraction: Double(position) / Double(duration))
                                HStack {
                                    Text(formatTime(ms: position))
                                    Spacer()
                                    Text("-" + formatTime(ms: duration - position))
                                }
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.white.opacity(0.7))
                            }
                            .frame(width: art)
                        }
                        HStack(spacing: 36) {
                            Button(action: model.toggleShuffle) {
                                Image(systemName: "shuffle")
                                    .foregroundStyle(player.shuffle == true ? AnyShapeStyle(.tint) : AnyShapeStyle(.white.opacity(0.7)))
                            }
                            Button(action: model.previous) { Image(systemName: "backward.fill").font(.title) }
                            Button(action: model.toggle) {
                                Image(systemName: player.playing == true ? "pause.fill" : "play.fill")
                                    .font(.system(size: 42))
                                    .frame(width: 48)
                            }
                            Button(action: model.next) { Image(systemName: "forward.fill").font(.title) }
                            Button(action: model.cycleRepeat) {
                                Image(systemName: player.repeat == "track" ? "repeat.1" : "repeat")
                                    .foregroundStyle(player.repeat == "off" || player.repeat == nil
                                        ? AnyShapeStyle(.white.opacity(0.7)) : AnyShapeStyle(.tint))
                            }
                        }
                        .buttonStyle(.plain)
                        .font(.title3)
                    }
                    if lyrics {
                        LyricsView(header: false, size: 30)
                            .frame(maxWidth: 560)
                            .frame(height: geometry.size.height * 0.8)
                            .mask(LinearGradient(
                                stops: [.init(color: .clear, location: 0), .init(color: .black, location: 0.12),
                                        .init(color: .black, location: 0.88), .init(color: .clear, location: 1)],
                                startPoint: .top, endPoint: .bottom))
                    }
                }
                .foregroundStyle(.white)
                .padding(48)
                VStack {
                    HStack {
                        Button { model.showNowPlaying = false } label: {
                            Image(systemName: "chevron.down").frame(width: 30, height: 30)
                        }
                        .buttonStyle(.glass)
                        .buttonBorderShape(.circle)
                        .keyboardShortcut(.escape, modifiers: [])
                        .help("Close")
                        Spacer()
                        Button { lyrics.toggle() } label: {
                            Image(systemName: "quote.bubble").frame(width: 30, height: 30)
                        }
                        .buttonStyle(.glass)
                        .buttonBorderShape(.circle)
                        .help(lyrics ? "Hide Lyrics" : "Show Lyrics")
                    }
                    Spacer()
                }
                .padding(.horizontal, 20)
                .padding(.top, 50)
            }
        }
        .environment(\.colorScheme, .dark)
        .task(id: player.uri) { await model.loadLyrics() }
    }
}

// MARK: Mini Player

/// Music's Mini Player: the cover filling a small window, its controls on
/// the cover while the pointer is over it, and the song beneath.
struct MiniPlayer: View {
    @Environment(AppModel.self) private var model
    @State private var hovered = false

    var body: some View {
        let player = model.player
        VStack(spacing: 0) {
            ZStack {
                Artwork(url: player.cover ?? player.image, size: 300, radius: 0)
                if hovered {
                    LinearGradient(colors: [.black.opacity(0.0), .black.opacity(0.55)],
                                   startPoint: .top, endPoint: .bottom)
                    HStack(spacing: 30) {
                        Button(action: model.previous) { Image(systemName: "backward.fill").font(.title2) }
                        Button(action: model.toggle) {
                            Image(systemName: player.playing == true ? "pause.fill" : "play.fill").font(.largeTitle)
                        }
                        Button(action: model.next) { Image(systemName: "forward.fill").font(.title2) }
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.white)
                    .padding(.top, 180)
                }
            }
            .frame(width: 300, height: 300)
            .clipped()
            .onHover { hovered = $0 }
            VStack(spacing: 6) {
                Text(player.title ?? "Nothing playing")
                    .font(.callout.weight(.semibold))
                    .lineLimit(1)
                Text(player.subtitle ?? "")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                TimelineView(.periodic(from: .now, by: 0.5)) { context in
                    let duration = max(player.durationMs ?? 0, 1)
                    SeekLine(fraction: Double(model.position(at: context.date)) / Double(duration))
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
        }
        .frame(width: 300)
        .background(.regularMaterial)
    }
}
