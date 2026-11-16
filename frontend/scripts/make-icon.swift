// Draws the app icon into an .iconset directory; package.sh turns it into AppIcon.icns with
// iconutil. Usage: swift make-icon.swift <out.iconset>
//
// The mark is Janus's gate: an ivory arch with a vertical seam and two eyes, one face looking
// each way. It stays readable at 16 px, where the arch is the whole silhouette.
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
    NSGradient(starting: NSColor(red: 0.83, green: 0.46, blue: 0.31, alpha: 1), ending: NSColor(red: 0.58, green: 0.25, blue: 0.15, alpha: 1))!.draw(in: shape, angle: -90)

    // The gate: a doorway whose top is a half circle, split down the middle.
    let cx = s / 2
    let w = s * 0.50, h = s * 0.54, r = w / 2
    let bottom = s * 0.22, spring = bottom + h - r
    let gate = NSBezierPath()
    gate.move(to: NSPoint(x: cx - r, y: bottom))
    gate.line(to: NSPoint(x: cx - r, y: spring))
    gate.appendArc(withCenter: NSPoint(x: cx, y: spring), radius: r, startAngle: 180, endAngle: 0, clockwise: true)
    gate.line(to: NSPoint(x: cx + r, y: bottom))
    gate.close()

    // Two faces: a seam and an eye on each side.
    let seam = NSRect(x: cx - s * 0.010, y: bottom + s * 0.02, width: s * 0.020, height: h - s * 0.06)
    let eyeY = bottom + h * 0.60
    for dx in [-1.0, 1.0] {
        let eye = NSRect(x: cx + CGFloat(dx) * s * 0.105 - s * 0.045, y: eyeY - s * 0.045, width: s * 0.090, height: s * 0.090)
        gate.append(NSBezierPath(ovalIn: eye))
    }
    gate.append(NSBezierPath(rect: seam))
    gate.windingRule = .evenOdd

    NSGraphicsContext.saveGraphicsState()
    let shadow = NSShadow()
    shadow.shadowOffset = NSSize(width: 0, height: -s * 0.012)
    shadow.shadowBlurRadius = s * 0.025
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.28)
    shadow.set()
    NSColor(red: 0.965, green: 0.941, blue: 0.894, alpha: 1).setFill()
    gate.fill()
    NSGraphicsContext.restoreGraphicsState()

    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128), ("128x128@2x", 256), ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
    try draw(px).write(to: out.appendingPathComponent("icon_\(name).png"))
}
