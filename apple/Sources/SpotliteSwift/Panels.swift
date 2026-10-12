import SwiftUI

// MARK: Queue

/// What plays next: the songs queued by hand, then the context's.
struct QueueView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let queue = model.queue
        List {
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
                            .contextMenu { SongMenu(tracks: [track], source: SongSource()) }
                    }
                    .onMove { from, to in
                        guard queue?.reorderable == true, let start = from.first else { return }
                        model.moveQueued(from: start, to: to)
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
        .safeAreaInset(edge: .top) { PanelHeader(title: "Queue") }
        .task { await model.loadQueue() }
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

    var body: some View {
        let lyrics = model.lyrics
        VStack(spacing: 0) {
            PanelHeader(title: "Lyrics")
            if let lyrics, !lyrics.lines.isEmpty, lyrics.uri == model.player.uri {
                TimelineView(.periodic(from: .now, by: 0.25)) { context in
                    let position = model.position(at: context.date)
                    let active = lyrics.synced ? activeLine(lyrics.lines, at: position) : nil
                    ScrollViewReader { proxy in
                        ScrollView {
                            VStack(alignment: .leading, spacing: 18) {
                                ForEach(Array(lyrics.lines.enumerated()), id: \.offset) { index, line in
                                    Text(line.text.isEmpty ? "♪" : line.text)
                                        .font(.system(size: 22, weight: .bold))
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
