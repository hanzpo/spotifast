import SwiftUI

/// Music's player: a Liquid Glass capsule with the transport at its start,
/// the song in the middle, and the volume at its end.
struct PlayerBar: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let player = model.player
        HStack(spacing: 18) {
            HStack(spacing: 14) {
                Button(action: model.toggleShuffle) {
                    Image(systemName: "shuffle")
                        .foregroundStyle(player.shuffle == true ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                }
                Button(action: model.previous) { Image(systemName: "backward.fill").font(.title3) }
                Button(action: model.toggle) {
                    Image(systemName: player.playing == true ? "pause.fill" : "play.fill")
                        .font(.title)
                        .frame(width: 28)
                }
                Button(action: model.next) { Image(systemName: "forward.fill").font(.title3) }
                Button(action: model.cycleRepeat) {
                    Image(systemName: player.repeat == "track" ? "repeat.1" : "repeat")
                        .foregroundStyle(player.repeat == "off" || player.repeat == nil
                            ? AnyShapeStyle(.secondary) : AnyShapeStyle(.tint))
                }
            }
            .buttonStyle(.plain)

            NowPlaying()
                .frame(maxWidth: .infinity)

            HStack(spacing: 8) {
                Image(systemName: (player.volume ?? 0) == 0 ? "speaker.slash.fill" : "speaker.wave.2.fill")
                    .foregroundStyle(.secondary)
                Slider(
                    value: Binding(
                        get: { Double(model.player.volume ?? 0) },
                        set: { model.setVolume(Int($0)) }),
                    in: 0...100
                )
                .controlSize(.mini)
                .frame(width: 90)
            }
        }
        .padding(.horizontal, 22)
        .frame(height: 58)
        .frame(maxWidth: 980)
        .glassEffect(.regular.interactive(), in: .capsule)
    }
}

/// The song: its cover, title and artists, and a thin line that seeks.
struct NowPlaying: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let player = model.player
        HStack(spacing: 10) {
            Artwork(url: player.image, size: 42, radius: 5)
            if player.hasSong {
                VStack(spacing: 2) {
                    Text(player.title ?? "")
                        .font(.callout.weight(.medium))
                        .lineLimit(1)
                    Text(player.subtitle ?? "")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    TimelineView(.periodic(from: .now, by: 0.5)) { context in
                        let duration = max(player.durationMs ?? 0, 1)
                        let position = model.scrubbing.map { Int($0 * Double(duration)) }
                            ?? model.position(at: context.date)
                        HStack(spacing: 8) {
                            Text(formatTime(ms: position))
                            SeekLine(fraction: Double(position) / Double(duration))
                            Text("-" + formatTime(ms: duration - position))
                        }
                        .font(.system(size: 10).monospacedDigit())
                        .foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity)
                Button(action: model.like) {
                    Image(systemName: player.saved == true ? "heart.fill" : "heart")
                        .foregroundStyle(player.saved == true ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                }
                .buttonStyle(.plain)
            } else {
                Text("Nothing playing")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
            }
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
