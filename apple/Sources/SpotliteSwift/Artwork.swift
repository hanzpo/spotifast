import AppKit
import SwiftUI

/// Covers fetched once and kept: `AsyncImage` asks again every time a row
/// scrolls back into view.
@MainActor
final class ImageCache {
    static let shared = ImageCache()

    private let memory = NSCache<NSURL, NSImage>()
    private var inFlight: [URL: Task<NSImage?, Never>] = [:]
    private let session: URLSession = {
        let configuration = URLSessionConfiguration.default
        configuration.urlCache = URLCache(
            memoryCapacity: 32 << 20, diskCapacity: 256 << 20,
            directory: FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("SpotliteSwift/images"))
        configuration.requestCachePolicy = .returnCacheDataElseLoad
        return URLSession(configuration: configuration)
    }()

    init() { memory.countLimit = 800 }

    func cached(_ url: URL) -> NSImage? { memory.object(forKey: url as NSURL) }

    func image(_ url: URL) async -> NSImage? {
        if let image = cached(url) { return image }
        if let task = inFlight[url] { return await task.value }
        let session = session
        let task = Task<NSImage?, Never> {
            guard let (data, _) = try? await session.data(from: url) else { return nil }
            return NSImage(data: data)
        }
        inFlight[url] = task
        let image = await task.value
        inFlight[url] = nil
        if let image { memory.setObject(image, forKey: url as NSURL) }
        return image
    }
}

/// Cover art from a URL, with a glyph in its place until it arrives.
struct Artwork: View {
    var url: String?
    var size: CGFloat
    var radius: CGFloat = 4
    var placeholder = "music.note"
    var circle = false

    @State private var image: NSImage?

    var body: some View {
        ZStack {
            if let image {
                Image(nsImage: image).resizable().scaledToFill()
            } else {
                Rectangle().fill(.quaternary)
                Image(systemName: placeholder)
                    .font(.system(size: max(size * 0.32, 9)))
                    .foregroundStyle(.tertiary)
            }
        }
        .frame(width: size, height: size)
        .clipShape(circle
            ? AnyShape(Circle())
            : AnyShape(RoundedRectangle(cornerRadius: radius, style: .continuous)))
        .task(id: url) {
            guard let url = url.flatMap(URL.init(string:)) else {
                image = nil
                return
            }
            if let cached = ImageCache.shared.cached(url) {
                image = cached
                return
            }
            image = await ImageCache.shared.image(url)
        }
    }
}

/// An image from a URL that fills whatever frame it is given, cropped.
struct FillImage: View {
    var url: String?
    @State private var image: NSImage?

    var body: some View {
        Rectangle()
            .fill(.quaternary)
            .overlay {
                if let image {
                    Image(nsImage: image).resizable().scaledToFill()
                }
            }
            .clipped()
            .task(id: url) {
                guard let url = url.flatMap(URL.init(string:)) else { return }
                image = ImageCache.shared.cached(url)
                if image == nil { image = await ImageCache.shared.image(url) }
            }
    }
}
