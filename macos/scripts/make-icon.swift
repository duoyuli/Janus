// Draws the app icon (a clay rounded square with a serif "J") into an .iconset directory;
// package.sh turns it into AppIcon.icns with iconutil. Usage: swift make-icon.swift <out.iconset>
import AppKit

let out = URL(fileURLWithPath: CommandLine.arguments[1])
try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

func draw(_ px: Int) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let s = CGFloat(px)
    // macOS icon grid: the shape fills about 80% of the canvas.
    let inset = s * 0.1
    let rect = NSRect(x: inset, y: inset, width: s - inset * 2, height: s - inset * 2)
    let shape = NSBezierPath(roundedRect: rect, xRadius: rect.width * 0.225, yRadius: rect.width * 0.225)
    NSGradient(starting: NSColor(red: 0.85, green: 0.45, blue: 0.31, alpha: 1), ending: NSColor(red: 0.71, green: 0.33, blue: 0.21, alpha: 1))!.draw(in: shape, angle: -90)
    let font = NSFont(name: "Georgia-Bold", size: rect.height * 0.62) ?? .boldSystemFont(ofSize: rect.height * 0.62)
    let text = NSAttributedString(string: "J", attributes: [.font: font, .foregroundColor: NSColor(red: 0.98, green: 0.97, blue: 0.95, alpha: 1)])
    let size = text.size()
    text.draw(at: NSPoint(x: rect.midX - size.width / 2, y: rect.midY - size.height / 2 + rect.height * 0.02))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128), ("128x128@2x", 256), ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
    try draw(px).write(to: out.appendingPathComponent("icon_\(name).png"))
}
