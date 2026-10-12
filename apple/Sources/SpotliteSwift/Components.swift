import SwiftUI

/// Asks the server for a page until it has finished loading, and again
/// whenever the library changes underneath it.
struct Loader<Value: Decodable & Sendable, Content: View>: View {
    @Environment(AppModel.self) private var model
    var method: String
    var params: [String: String] = [:]
    var loading: (Value) -> Bool
    @ViewBuilder var content: (Value, _ more: @escaping () -> Void) -> Content

    @State private var value: Value?
    @State private var failure: String?
    @State private var generation = 0

    private var key: String {
        "\(method)|\(params.sorted { $0.key < $1.key })|\(model.revision)|\(generation)"
    }

    var body: some View {
        Group {
            if let value {
                content(value, more)
            } else if let failure {
                ContentUnavailableView(
                    "Couldn't load this", systemImage: "exclamationmark.triangle",
                    description: Text(failure))
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task(id: key) { await poll() }
    }

    private func poll() async {
        for attempt in 0..<80 {
            do {
                let next = try await model.call(method, params, as: Value.self)
                value = next
                failure = nil
                if !loading(next) { return }
            } catch {
                if value == nil { failure = error.localizedDescription }
                return
            }
            try? await Task.sleep(for: .milliseconds(attempt < 10 ? 200 : 700))
            if Task.isCancelled { return }
        }
    }

    /// Asks for the next page of a long list, then follows it in.
    private func more() {
        guard let page = params["page"] ?? pageName else { return }
        model.send("page.more", ["page": page])
        Task {
            try? await Task.sleep(for: .milliseconds(150))
            generation += 1
        }
    }

    private var pageName: String? {
        switch method {
        case "library.liked": "liked"
        case "library.albums": "albums"
        case "library.artists": "artists"
        case "library.shows": "podcasts"
        case "library.episodes": "episodes"
        case "playlist.get": params["id"].map { "playlist:\($0)" }
        case "album.get": params["id"].map { "album:\($0)" }
        case "show.get": params["id"].map { "show:\($0)" }
        default: nil
        }
    }
}

/// A card in a grid or a shelf: the cover, the name, and a line under it.
struct CardView: View {
    @Environment(AppModel.self) private var model
    var card: Card
    var size: CGFloat = 168

    @State private var hovered = false

    var body: some View {
        VStack(alignment: card.round ? .center : .leading, spacing: 6) {
            ZStack(alignment: .bottomTrailing) {
                Artwork(
                    url: card.image, size: size, radius: 8,
                    placeholder: card.round ? "person.fill" : "music.note",
                    circle: card.round)
                    .shadow(color: .black.opacity(hovered ? 0.3 : 0.15), radius: hovered ? 10 : 6, y: 4)
                if hovered, card.kind != "artist", card.kind != "user" {
                    Button {
                        model.play(context: card.uri)
                    } label: {
                        Image(systemName: "play.fill")
                            .font(.system(size: 14, weight: .semibold))
                            .frame(width: 34, height: 34)
                    }
                    .buttonStyle(.glass)
                    .buttonBorderShape(.circle)
                    .padding(8)
                }
            }
            Text(card.name)
                .font(.callout.weight(.medium))
                .lineLimit(1)
            if let subtitle = card.subtitle, !subtitle.isEmpty {
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .multilineTextAlignment(card.round ? .center : .leading)
            }
        }
        .frame(width: size, alignment: card.round ? .center : .leading)
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture { if let route = card.route { model.open(route) } }
        .contextMenu { CardMenu(card: card) }
    }
}

struct CardMenu: View {
    @Environment(AppModel.self) private var model
    var card: Card

    var body: some View {
        if card.kind != "artist", card.kind != "user" {
            Button("Play") { model.play(context: card.uri) }
            Button("Shuffle") { model.play(context: card.uri, shuffle: true) }
            Divider()
        }
        if let route = card.route {
            Button("Open") { model.open(route) }
        }
        if card.kind == "album" || card.kind == "playlist" || card.kind == "show" {
            Button("Save to / Remove from Library") { model.toggleLibrary(uri: card.uri) }
        }
        if card.kind == "artist" || card.kind == "playlist" || card.kind == "album" {
            Button("Start Radio") { model.startRadio(seed: card.uri) }
        }
        Divider()
        Button("Copy Link") { model.copyLink(card.uri) }
    }
}

/// Cards wrapped across the width.
struct CardGrid: View {
    var cards: [Card]
    var more: (() -> Void)?

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 160, maximum: 200), spacing: 20, alignment: .top)],
                  alignment: .leading, spacing: 24) {
            ForEach(cards) { card in
                CardView(card: card, size: 160)
                    .onAppear {
                        if card.id == cards.last?.id { more?() }
                    }
            }
        }
    }
}

/// A titled row of cards that scrolls sideways, as Music's shelves do.
struct ShelfView: View {
    @Environment(AppModel.self) private var model
    var title: String
    var cards: [Card]
    var seeAll: Route?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Text(title).font(.title2.weight(.bold))
                Spacer()
                if let seeAll {
                    Button("See All") { model.open(seeAll) }
                        .buttonStyle(.link)
                }
            }
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: 20) {
                    ForEach(cards) { card in CardView(card: card) }
                }
                .scrollTargetLayout()
            }
            .scrollIndicators(.never)
            .scrollTargetBehavior(.viewAligned)
        }
    }
}

/// Music's page header: the cover, and beside it the title in bold, who
/// made it in the accent, a line of facts, the description and actions.
struct PageHeader<Actions: View>: View {
    @Environment(AppModel.self) private var model
    var cover: String?
    var title: String
    var owner: String?
    var ownerRoute: Route?
    var facts: [String] = []
    var description: String?
    var round = false
    /// A library list's own cover: a symbol on the accent.
    var glyph: String?
    @ViewBuilder var actions: Actions

    var body: some View {
        // A narrow window, with a panel open, takes the smaller cover and
        // the buttons' icons alone.
        ViewThatFits(in: .horizontal) {
            layout(cover: 230, compact: false)
            layout(cover: 150, compact: true)
        }
        .padding(.bottom, 24)
    }

    private func layout(cover size: CGFloat, compact: Bool) -> some View {
        HStack(alignment: .bottom, spacing: compact ? 20 : 28) {
            Group {
                if let glyph {
                    LikedCover(size: size, symbol: glyph)
                } else {
                    Artwork(url: cover, size: size, radius: 10,
                            placeholder: round ? "person.fill" : "music.note", circle: round)
                }
            }
            .shadow(color: .black.opacity(0.25), radius: 16, y: 8)
            VStack(alignment: .leading, spacing: 6) {
                Text(title)
                    .font(.system(size: 28, weight: .bold))
                    .lineLimit(2)
                    .textSelection(.enabled)
                if let owner, !owner.isEmpty {
                    Button(owner) { if let ownerRoute { model.open(ownerRoute) } }
                        .buttonStyle(.plain)
                        .font(.system(size: 20))
                        .foregroundStyle(.tint)
                        .disabled(ownerRoute == nil)
                }
                if !facts.isEmpty {
                    Text(facts.joined(separator: " · ").uppercased())
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                }
                if let description, !description.isEmpty {
                    Text(description)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                }
                GlassEffectContainer {
                    HStack(spacing: 10) { actions }
                        .buttonStyle(.glass)
                        .controlSize(.large)
                        .foregroundStyle(.tint)
                        .fontWeight(.semibold)
                        .labelStyle(compact ? AnyLabelStyle(.iconOnly) : AnyLabelStyle(.titleAndIcon))
                }
                .padding(.top, 12)
            }
            .frame(minWidth: compact ? 0 : 320, alignment: .leading)
            Spacer(minLength: 0)
        }
    }
}

/// Play and Shuffle, as every page of songs offers them.
struct PlayButtons: View {
    @Environment(AppModel.self) private var model
    var context: String?

    var body: some View {
        Button {
            if let context { model.play(context: context, shuffle: false) }
        } label: {
            Label("Play", systemImage: "play.fill").frame(minWidth: 70)
        }
        .disabled(context == nil)
        Button {
            if let context { model.play(context: context, shuffle: true) }
        } label: {
            Label("Shuffle", systemImage: "shuffle").frame(minWidth: 70)
        }
        .disabled(context == nil)
    }
}

struct LikedCover: View {
    var size: CGFloat
    var symbol = "heart.fill"

    var body: some View {
        RoundedRectangle(cornerRadius: 10, style: .continuous)
            .fill(LinearGradient(
                colors: [.accentColor, .accentColor.opacity(0.55)],
                startPoint: .topLeading, endPoint: .bottomTrailing))
            .overlay(Image(systemName: symbol)
                .font(.system(size: size / 3))
                .foregroundStyle(.white))
            .frame(width: size, height: size)
    }
}

/// A page's scrolling body, clear of the player at its foot.
struct PageScroll<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) { content }
                .padding(.horizontal, 28)
                .padding(.top, 12)
                .padding(.bottom, 110)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// A big bold page title, for pages without a header.
struct PageTitle: View {
    var text: String

    var body: some View {
        Text(text)
            .font(.system(size: 28, weight: .bold))
            .padding(.bottom, 18)
    }
}

struct LoadingFooter: View {
    var loading: Bool
    var error: String?

    var body: some View {
        if loading {
            ProgressView().controlSize(.small).frame(maxWidth: .infinity).padding()
        } else if let error {
            Text(error).foregroundStyle(.secondary).frame(maxWidth: .infinity).padding()
        }
    }
}

/// A label style chosen at run time.
struct AnyLabelStyle: LabelStyle {
    private let make: (Configuration) -> AnyView

    init(_ style: some LabelStyle) {
        make = { AnyView(style.makeBody(configuration: $0)) }
    }

    func makeBody(configuration: Configuration) -> some View { make(configuration) }
}

extension View {
    /// A menu dressed as the glass buttons beside it: menus take no
    /// button style of their own.
    func glassMenu() -> some View {
        self
            .fixedSize()
            .frame(width: 44, height: 30)
            .contentShape(Capsule())
            .glassEffect(.regular.interactive(), in: .capsule)
    }
}
