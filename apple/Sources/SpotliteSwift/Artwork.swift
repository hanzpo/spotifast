import AppKit
import ImageIO
import SwiftUI

/// Covers fetched once, kept on disk, and held in memory only at the size
/// they are drawn at.
///
/// A cover decoded at full size costs its whole bitmap however small it is
/// shown: 640 pixels square is 1.6 MB, and a library holds hundreds. Each
/// cover is decoded at one of a few sizes just above what the view draws,
/// the decoded bitmaps share a budget, and the downloads stay on disk, so
/// a cover the budget let go comes back without the network.
@MainActor
final class ImageCache {
    static let shared = ImageCache()

    /// Decoded covers, by URL and size step, costed by their bitmap's bytes.
    private let memory = NSCache<NSString, NSImage>()
    private var inFlight: [String: Task<NSImage?, Never>] = [:]
    private let session: URLSession = {
        let configuration = URLSessionConfiguration.default
        configuration.urlCache = URLCache(
            // The downloads in memory are only a short-term buffer; the
            // disk keeps them.
            memoryCapacity: 4 << 20, diskCapacity: 512 << 20,
            directory: FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("SpotliteSwift/images"))
        configuration.requestCachePolicy = .returnCacheDataElseLoad
        return URLSession(configuration: configuration)
    }()

    init() {
        memory.totalCostLimit = 64 << 20
    }

    /// The size steps covers are decoded at, in pixels, so views of
    /// nearly the same size share one bitmap. A shelf card on a Retina
    /// screen is about 340 pixels across: decoded at 384, not 512, it
    /// costs little more than half.
    private static let steps = [64, 128, 192, 256, 384, 512, 768, 1024, 2048]

    private static func step(for pixels: CGFloat) -> Int {
        steps.first { CGFloat($0) >= pixels } ?? steps[steps.count - 1]
    }

    private static func key(_ url: URL, _ step: Int) -> NSString {
        "\(step)|\(url.absoluteString)" as NSString
    }

    func cached(_ url: URL, pixels: CGFloat) -> NSImage? {
        memory.object(forKey: Self.key(url, Self.step(for: pixels)))
    }

    func image(_ url: URL, pixels: CGFloat) async -> NSImage? {
        let step = Self.step(for: pixels)
        let key = Self.key(url, step)
        if let image = memory.object(forKey: key) { return image }
        if let task = inFlight[key as String] { return await task.value }
        let session = session
        let task = Task<NSImage?, Never>.detached(priority: .utility) {
            guard let (data, _) = try? await session.data(from: url) else { return nil }
            return Self.decode(data, maxPixels: step)
        }
        inFlight[key as String] = task
        let image = await task.value
        inFlight[key as String] = nil
        if let image, let cost = image.representations.first.map({ $0.pixelsWide * $0.pixelsHigh * 4 }) {
            memory.setObject(image, forKey: key, cost: cost)
        }
        return image
    }

    /// The image decoded straight to a bitmap no larger than `maxPixels`
    /// on its longer side.
    nonisolated private static func decode(_ data: Data, maxPixels: Int) -> NSImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixels,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
            return nil
        }
        return NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
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
    @Environment(\.displayScale) private var scale

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
            let pixels = size * scale
            if let cached = ImageCache.shared.cached(url, pixels: pixels) {
                image = cached
                return
            }
            image = await ImageCache.shared.image(url, pixels: pixels)
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
            // Drawn across a page: decoded as wide as a large window.
            .task(id: url) {
                guard let url = url.flatMap(URL.init(string:)) else { return }
                image = ImageCache.shared.cached(url, pixels: 2048)
                if image == nil { image = await ImageCache.shared.image(url, pixels: 2048) }
            }
    }
}
