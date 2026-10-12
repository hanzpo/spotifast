import SwiftUI

/// A playlist or Liked Songs as Music shows an album: the cover and its
/// words, the actions, then the songs on alternating stripes.
struct DetailView: View {
    @Environment(AppModel.self) private var model
    var detail: Detail
    @State private var selected: Track.ID?

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                header
                    .padding(.bottom, 24)
                ColumnHeader()
                ForEach(detail.tracks) { track in
                    SongRow(
                        track: track,
                        striped: track.position % 2 == 1,
                        selected: selected == track.id,
                        playing: model.player.uri == track.uri
                    )
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) { model.play(track) }
                    .simultaneousGesture(TapGesture().onEnded { selected = track.id })
                }
                if detail.loading {
                    ProgressView().frame(maxWidth: .infinity).padding()
                }
                if let error = detail.error {
                    Text(error).foregroundStyle(.secondary).padding()
                }
            }
            .padding(.horizontal, 28)
            .padding(.top, 8)
            .padding(.bottom, 96)
        }
    }

    private var header: some View {
        HStack(alignment: .bottom, spacing: 28) {
            if detail.cover != nil || detail.context?.hasSuffix(":collection") == false {
                Artwork(url: detail.cover, size: 240, radius: 10)
                    .shadow(color: .black.opacity(0.25), radius: 16, y: 8)
            } else {
                LikedCover(size: 240)
            }
            VStack(alignment: .leading, spacing: 6) {
                Text(detail.title)
                    .font(.system(size: 28, weight: .bold))
                    .lineLimit(2)
                if let owner = detail.owner {
                    Text(owner)
                        .font(.system(size: 20))
                        .foregroundStyle(.tint)
                }
                Text(detail.facts)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                if let description = detail.description, !description.isEmpty {
                    Text(description)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                GlassEffectContainer {
                    HStack(spacing: 10) {
                        Button { model.playDetail(shuffle: false) } label: {
                            Label("Play", systemImage: "play.fill").frame(minWidth: 72)
                        }
                        Button { model.playDetail(shuffle: true) } label: {
                            Label("Shuffle", systemImage: "shuffle").frame(minWidth: 72)
                        }
                    }
                    .buttonStyle(.glass)
                    .controlSize(.large)
                    .foregroundStyle(.tint)
                    .fontWeight(.semibold)
                }
                .padding(.top, 14)
            }
            Spacer(minLength: 0)
        }
    }
}

struct LikedCover: View {
    var size: CGFloat

    var body: some View {
        RoundedRectangle(cornerRadius: 10, style: .continuous)
            .fill(LinearGradient(
                colors: [.accentColor, .accentColor.opacity(0.55)],
                startPoint: .topLeading, endPoint: .bottomTrailing))
            .overlay(Image(systemName: "heart.fill").font(.system(size: size / 3)).foregroundStyle(.white))
            .frame(width: size, height: size)
            .shadow(color: .black.opacity(0.25), radius: 16, y: 8)
    }
}

private enum Columns {
    static let number: CGFloat = 34
    static let time: CGFloat = 56
}

struct ColumnHeader: View {
    var body: some View {
        HStack(spacing: 12) {
            Text("#").frame(width: Columns.number)
            Text("Title").frame(maxWidth: .infinity, alignment: .leading)
            Text("Album").frame(maxWidth: .infinity, alignment: .leading)
            Text("Time").frame(width: Columns.time, alignment: .trailing)
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
    var track: Track
    var striped: Bool
    var selected: Bool
    var playing: Bool

    var body: some View {
        HStack(spacing: 12) {
            Group {
                if playing {
                    Image(systemName: "speaker.wave.2.fill").foregroundStyle(selected ? .white : Color.accentColor)
                } else {
                    Text("\(track.position + 1)").foregroundStyle(selected ? .white.opacity(0.8) : .secondary)
                }
            }
            .frame(width: Columns.number)
            HStack(spacing: 10) {
                Artwork(url: track.image, size: 36)
                VStack(alignment: .leading, spacing: 1) {
                    Text(track.name)
                        .fontWeight(.medium)
                        .lineLimit(1)
                    Text(track.artistNames)
                        .font(.callout)
                        .foregroundStyle(selected ? .white.opacity(0.85) : .secondary)
                        .lineLimit(1)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            Text(track.albumName)
                .foregroundStyle(selected ? .white.opacity(0.85) : .secondary)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
            Text(track.duration)
                .monospacedDigit()
                .foregroundStyle(selected ? .white.opacity(0.85) : .secondary)
                .frame(width: Columns.time, alignment: .trailing)
        }
        .foregroundStyle(selected ? .white : .primary)
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background {
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(selected ? AnyShapeStyle(.tint) : striped ? AnyShapeStyle(.quinary) : AnyShapeStyle(.clear))
        }
    }
}
