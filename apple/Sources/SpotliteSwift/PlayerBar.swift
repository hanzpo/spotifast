import SwiftUI

/// Music's player: a Liquid Glass capsule with the transport at its start,
/// the song in the middle, and the panels, devices and volume at its end.
struct PlayerBar: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        let player = model.player
        HStack(spacing: 16) {
            HStack(spacing: 12) {
                Button(action: model.toggleShuffle) {
                    Image(systemName: "shuffle")
                        .foregroundStyle(player.shuffle == true ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                }
                .help("Shuffle")
                Button(action: model.previous) { Image(systemName: "backward.fill").font(.title3) }
                    .help("Previous")
                Button(action: model.toggle) {
                    Group {
                        if player.loading == true {
                            ProgressView().controlSize(.small)
                        } else {
                            Image(systemName: player.playing == true ? "pause.fill" : "play.fill")
                                .font(.title)
                        }
                    }
                    .frame(width: 30, height: 30)
                }
                .help(player.playing == true ? "Pause" : "Play")
                Button(action: model.next) { Image(systemName: "forward.fill").font(.title3) }
                    .help("Next")
                Button(action: model.cycleRepeat) {
                    Image(systemName: player.repeat == "track" ? "repeat.1" : "repeat")
                        .foregroundStyle(player.repeat == "off" || player.repeat == nil
                            ? AnyShapeStyle(.secondary) : AnyShapeStyle(.tint))
                }
                .help("Repeat")
            }
            .buttonStyle(.plain)
            .disabled(!player.hasSong)

            NowPlaying()
                .frame(maxWidth: .infinity)

            HStack(spacing: 12) {
                KaraokeButton()
                Button { model.togglePanel(.lyrics) } label: {
                    Image(systemName: "quote.bubble")
                        .foregroundStyle(model.panel == .lyrics ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                }
                .help("Lyrics")
                Button { model.togglePanel(.queue) } label: {
                    Image(systemName: "list.bullet")
                        .foregroundStyle(model.panel == .queue ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                }
                .help("Queue")
                Button { model.showDevices.toggle() } label: {
                    Image(systemName: "airplayaudio")
                        .foregroundStyle(player.local == false ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                }
                .help(player.device.map { "Playing on \($0)" } ?? "Play On")
                .popover(isPresented: $model.showDevices, arrowEdge: .top) { DevicesView() }
                // The slider gives way first when the window is narrow.
                ViewThatFits(in: .horizontal) {
                    Volume(slider: true)
                    Volume(slider: false)
                }
                .frame(minWidth: 22)
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 22)
        .frame(height: 60)
        .frame(maxWidth: 1000)
        .glassEffect(.regular.interactive(), in: .capsule)
    }
}

/// Karaoke's microphone: takes the voice out of the song on this Mac.
/// `glass` draws it round, as the full-window player's buttons are.
struct KaraokeButton: View {
    @Environment(AppModel.self) private var model
    var glass = false

    var body: some View {
        let player = model.player
        if let karaoke = player.karaoke, karaoke.available, player.local != false {
            let button = Button(action: model.toggleKaraoke) {
                Group {
                    if karaoke.busy {
                        ProgressView().controlSize(.mini)
                    } else {
                        Image(systemName: "music.mic")
                            .foregroundStyle(karaoke.on ? AnyShapeStyle(.tint) : AnyShapeStyle(glass ? .primary : .secondary))
                    }
                }
                .frame(width: glass ? 30 : 16, height: glass ? 30 : nil)
            }
            .help(Self.help(karaoke))
            .disabled(!player.hasSong && !karaoke.on)
            if glass {
                button.buttonStyle(.glass).buttonBorderShape(.circle)
            } else {
                button.buttonStyle(.plain)
            }
        }
    }

    static func help(_ karaoke: KaraokeState) -> String {
        switch karaoke.status {
        case "downloading": "Downloading the vocal model…"
        case "preparing": "Preparing karaoke…"
        case "failed": "Karaoke couldn't start"
        default: karaoke.on ? "Turn Off Karaoke" : "Karaoke"
        }
    }
}

/// How much of the voice karaoke keeps, shown while it is on.
struct VocalSlider: View {
    @Environment(AppModel.self) private var model
    @State private var dragging: Double?
    var width: CGFloat = 120

    var body: some View {
        if let karaoke = model.player.karaoke, karaoke.on, model.player.local != false {
            HStack(spacing: 6) {
                Image(systemName: "person.wave.2")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Slider(
                    value: Binding(
                        get: { dragging ?? karaoke.level },
                        set: { value in
                            dragging = value
                            model.setVocalLevel(value, done: false)
                        }),
                    in: 0...1
                ) { editing in
                    if !editing, let value = dragging {
                        model.setVocalLevel(value, done: true)
                        dragging = nil
                    }
                }
                .controlSize(.mini)
                .frame(width: width)
            }
            .help("Vocals")
            .transition(.opacity.combined(with: .move(edge: .trailing)))
        }
    }
}

struct Volume: View {
    @Environment(AppModel.self) private var model
    var slider = true
    @State private var dragging: Double?

    var body: some View {
        let volume = model.player.volume ?? 0
        HStack(spacing: 6) {
            Button(action: model.toggleMute) {
                Image(systemName: volumeSymbol(dragging.map(Int.init) ?? volume))
                    .frame(width: 22)
                    .foregroundStyle(.secondary)
            }
            .help(volume == 0 ? "Unmute" : "Mute")
            if slider {
            Slider(
                value: Binding(
                    get: { dragging ?? Double(volume) },
                    set: { value in
                        dragging = value
                        model.setVolume(Int(value), done: false)
                    }),
                in: 0...100
            ) { editing in
                if !editing, let value = dragging {
                    model.setVolume(Int(value), done: true)
                    dragging = nil
                }
            }
            .controlSize(.mini)
            .frame(width: 84)
            .disabled(model.player.canSetVolume == false)
            }
        }
    }

    private func volumeSymbol(_ volume: Int) -> String {
        switch volume {
        case 0: "speaker.slash.fill"
        case 1..<34: "speaker.wave.1.fill"
        case 34..<67: "speaker.wave.2.fill"
        default: "speaker.wave.3.fill"
        }
    }
}

/// The song: its cover, title and artists, and a thin line that seeks.
struct NowPlaying: View {
    @Environment(AppModel.self) private var model
    @State private var hovered = false

    var body: some View {
        let player = model.player
        HStack(spacing: 10) {
            Artwork(url: player.image, size: 44, radius: 5)
                .onTapGesture { if player.hasSong { model.showNowPlaying = true } }
                .help("Show Now Playing")
                .draggable(player.uri ?? "")
            if player.hasSong {
                VStack(spacing: 1) {
                    Button(player.title ?? "") { openSource() }
                        .buttonStyle(.plain)
                        .font(.callout.weight(.semibold))
                        .lineLimit(1)
                    subtitle
                    TimelineView(.periodic(from: .now, by: 0.5)) { context in
                        let duration = max(player.durationMs ?? 0, 1)
                        let position = model.scrubbing.map { Int($0 * Double(duration)) }
                            ?? model.position(at: context.date)
                        HStack(spacing: 8) {
                            Text(formatTime(ms: position))
                                .opacity(hovered || model.scrubbing != nil ? 1 : 0)
                            SeekLine(fraction: Double(position) / Double(duration))
                            Text("-" + formatTime(ms: duration - position))
                                .opacity(hovered || model.scrubbing != nil ? 1 : 0)
                        }
                        .font(.system(size: 10).monospacedDigit())
                        .foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity)
                if player.isEpisode != true {
                    Button {
                        if let uri = player.uri { model.toggleSaved(uri) }
                    } label: {
                        Image(systemName: player.saved == true ? "heart.fill" : "heart")
                            .foregroundStyle(player.saved == true ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                    }
                    .buttonStyle(.plain)
                    .help(player.saved == true ? "Remove from Liked Songs" : "Save to Liked Songs")
                }
            } else {
                Text("Nothing playing")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
            }
        }
        .onHover { hovered = $0 }
        .contextMenu {
            if let uri = player.uri, let title = player.title {
                let track = Track(
                    kind: player.isEpisode == true ? "episode" : "track", uri: uri, name: title,
                    artists: player.artists ?? [], album: player.album, durationMs: player.durationMs ?? 0,
                    explicit: false, image: player.image, showId: player.showId)
                SongMenu(tracks: [track], source: SongSource())
                if let page = player.playingFromPage, let route = Route(page: page) {
                    Divider()
                    Button("Go to \(player.playingFrom ?? "Playing From")") { model.open(route) }
                }
            }
        }
    }

    @ViewBuilder private var subtitle: some View {
        let player = model.player
        HStack(spacing: 0) {
            if let artists = player.artists, !artists.isEmpty {
                ForEach(Array(artists.enumerated()), id: \.offset) { index, artist in
                    if index > 0 { Text(", ") }
                    Button(artist.name) { if let id = artist.id { model.open(.artist(id)) } }
                }
            } else {
                Button(player.subtitle ?? "") { if let show = player.showId { model.open(.show(show)) } }
            }
            if let from = player.playingFrom, hovered {
                Text(" — \(from)")
            }
        }
        .buttonStyle(.plain)
        .font(.caption)
        .foregroundStyle(.secondary)
        .lineLimit(1)
    }

    private func openSource() {
        if let album = model.player.album?.id {
            model.open(.album(album))
        } else if let show = model.player.showId {
            model.open(.show(show))
        }
    }
}

/// Music's position line: thin, no knob, seeks where pressed or dragged.
struct SeekLine: View {
    @Environment(AppModel.self) private var model
    var fraction: Double

    var body: some View {
        GeometryReader { geometry in
            let shown = model.scrubbing ?? fraction
            ZStack(alignment: .leading) {
                Capsule().fill(.quaternary)
                Capsule().fill(model.scrubbing == nil ? AnyShapeStyle(.secondary) : AnyShapeStyle(.primary))
                    .frame(width: geometry.size.width * min(max(shown, 0), 1))
            }
            .frame(height: model.scrubbing == nil ? 3 : 5)
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        model.scrubbing = min(max(value.location.x / geometry.size.width, 0), 1)
                    }
                    .onEnded { value in
                        model.seek(to: min(max(value.location.x / geometry.size.width, 0), 1))
                        model.scrubbing = nil
                    }
            )
        }
        .frame(height: 10)
    }
}
