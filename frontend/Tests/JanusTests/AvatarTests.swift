import AppKit
import Testing
@testable import Janus

/// A picture for a digital employee is the centered square of the image, scaled to 256 pixels
/// as PNG; something that is not an image is refused (docs/domains/20261031-agents_and_roles.md#avatars).
@Test func aPictureIsTheCenteredSquareScaledDown() throws {
    // A wide image: red on the left third, green in the middle, blue on the right.
    let rep = try #require(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 300, pixelsHigh: 100, bitsPerSample: 8, samplesPerPixel: 4,
                                            hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
    let red = NSColor(deviceRed: 1, green: 0, blue: 0, alpha: 1), green = NSColor(deviceRed: 0, green: 1, blue: 0, alpha: 1)
    let blue = NSColor(deviceRed: 0, green: 0, blue: 1, alpha: 1)
    for x in 0..<300 {
        for y in 0..<100 { rep.setColor(x < 100 ? red : x < 200 ? green : blue, atX: x, y: y) }
    }
    let source = try #require(rep.representation(using: .png, properties: [:]))
    let png = try #require(AvatarImages.square(source))
    #expect(png.starts(with: [0x89, 0x50, 0x4E, 0x47]))
    let out = try #require(NSBitmapImageRep(data: png))
    #expect(out.pixelsWide == 256 && out.pixelsHigh == 256)
    let mid = try #require(out.colorAt(x: 128, y: 128)?.usingColorSpace(.deviceRGB))
    #expect(mid.greenComponent > 0.9 && mid.redComponent < 0.1, "the middle of the image, not its left edge")
    let edge = try #require(out.colorAt(x: 2, y: 128)?.usingColorSpace(.deviceRGB))
    #expect(edge.greenComponent > 0.9, "the sides are cut off")
    #expect(AvatarImages.square(Data("not an image".utf8)) == nil)
}
