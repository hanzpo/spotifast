// Renders AppIcon.svg to the 1024 px PNG the bundle's icon is made from:
//
//   swift apple/Packaging/render-icon.swift
import AppKit

let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
guard let image = NSImage(contentsOf: here.appendingPathComponent("AppIcon.svg")) else {
    fatalError("AppIcon.svg did not load")
}
let size = 1024
let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
    samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
    bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
image.draw(in: NSRect(x: 0, y: 0, width: size, height: size))
NSGraphicsContext.restoreGraphicsState()
try bitmap.representation(using: .png, properties: [:])!
    .write(to: here.appendingPathComponent("AppIcon-1024.png"))
