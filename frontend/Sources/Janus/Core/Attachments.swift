import AppKit
import UniformTypeIdentifiers

/// An image waiting to be sent with a goal or a message. It goes to the server as base64
/// (docs/interfaces/http_api.md#images), which stores it with the run.
struct PastedImage: Identifiable, Equatable {
    let id = UUID()
    let data: Data
    let mimeType: String
    let name: String

    var json: JSON { ["data": JSON(data.base64EncodedString()), "name": JSON(name), "mimeType": JSON(mimeType)] }

    static func == (a: PastedImage, b: PastedImage) -> Bool { a.id == b.id }
}

enum Attach {
    /// At most this many images per goal or message (the server refuses more).
    static let maxImages = 10
    /// Larger images are scaled down and re-encoded, so that each one stays within what the
    /// agents' models accept (5 MB of base64 for Claude).
    static let maxBytes = 3_750_000
    static let maxSide: CGFloat = 2048

    /// The type of an image file, from its first bytes; nil when it is none the server takes.
    static func sniff(_ d: Data) -> String? {
        let b = [UInt8](d.prefix(12))
        if b.starts(with: [0x89, 0x50, 0x4E, 0x47]) { return "image/png" }
        if b.starts(with: [0xFF, 0xD8, 0xFF]) { return "image/jpeg" }
        if b.starts(with: Array("GIF8".utf8)) { return "image/gif" }
        if b.count >= 12, b.starts(with: Array("RIFF".utf8)), Array(b[8..<12]) == Array("WEBP".utf8) { return "image/webp" }
        return nil
    }

    /// An image ready to send: kept as is when it is a supported type of a reasonable size,
    /// otherwise converted to PNG, or scaled down to JPEG when PNG is still too large.
    static func prepare(_ data: Data?, name: String) -> PastedImage? {
        guard let data, !data.isEmpty else { return nil }
        if let mime = sniff(data), data.count <= maxBytes { return PastedImage(data: data, mimeType: mime, name: name) }
        guard let image = NSImage(data: data), let rep = bitmap(image, maxSide: nil) else { return nil }
        let base = (name as NSString).deletingPathExtension
        if let png = rep.representation(using: .png, properties: [:]), png.count <= maxBytes {
            return PastedImage(data: png, mimeType: "image/png", name: base + ".png")
        }
        guard let small = bitmap(image, maxSide: maxSide), let jpeg = small.representation(using: .jpeg, properties: [.compressionFactor: 0.85]) else { return nil }
        return PastedImage(data: jpeg, mimeType: "image/jpeg", name: base + ".jpg")
    }

    /// The image drawn into a bitmap, its longest side at most `maxSide` pixels when given.
    private static func bitmap(_ image: NSImage, maxSide: CGFloat?) -> NSBitmapImageRep? {
        guard let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return nil }
        var w = CGFloat(cg.width), h = CGFloat(cg.height)
        if let maxSide, max(w, h) > maxSide { let k = maxSide / max(w, h); w = (w * k).rounded(); h = (h * k).rounded() }
        guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(w), pixelsHigh: Int(h), bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
              let ctx = NSGraphicsContext(bitmapImageRep: rep) else { return nil }
        ctx.cgContext.interpolationQuality = .high
        ctx.cgContext.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))
        return rep
    }

    /// The images a paste or a drop brings: image files (copied in Finder or dragged in), or
    /// image data on its own (a screenshot, "Copy Image" in a browser). Empty when the
    /// pasteboard holds text or other files, so those paste as before.
    static func images(from pb: NSPasteboard) -> [PastedImage] {
        if let urls = pb.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL], !urls.isEmpty {
            let images = urls.filter { UTType(filenameExtension: $0.pathExtension)?.conforms(to: .image) == true }
            guard images.count == urls.count else { return [] }
            return images.compactMap { prepare(try? Data(contentsOf: $0), name: $0.lastPathComponent) }
        }
        // Text wins: a spreadsheet or a rich document puts a picture of the text next to it.
        // A lone link does not count: browsers put the image's URL on the pasteboard for
        // "Copy Image", and the person means the picture.
        if takesText(pb) { return [] }
        for (type, ext) in [(NSPasteboard.PasteboardType.png, "png"), (NSPasteboard.PasteboardType("public.jpeg"), "jpg"), (.tiff, "png")] {
            if let d = pb.data(forType: type), let img = prepare(d, name: "粘贴的图片.\(ext)") { return [img] }
        }
        return []
    }

    /// Whether a paste or a drop would bring images, without reading them.
    static func hasImages(_ pb: NSPasteboard) -> Bool {
        if let urls = pb.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL], !urls.isEmpty {
            return urls.allSatisfy { UTType(filenameExtension: $0.pathExtension)?.conforms(to: .image) == true }
        }
        return !takesText(pb) && pb.availableType(from: [.png, .tiff, NSPasteboard.PasteboardType("public.jpeg")]) != nil
    }

    /// Whether the pasteboard's text is content the person meant to paste. Empty text and a
    /// lone link are not: they accompany an image copied from a browser.
    private static func takesText(_ pb: NSPasteboard) -> Bool {
        guard let s = pb.string(forType: .string) else { return false }
        let text = s.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty { return false }
        return text.range(of: #"^[a-zA-Z][a-zA-Z0-9+.-]*://\S+$"#, options: .regularExpression) == nil
    }

    /// The person's words of a message, without the lines Janus added to name its images
    /// (`[附图 N] <path>`, see backend/src/attachments.rs); the images are shown instead.
    static func visibleText(_ text: String) -> String {
        let lines = text.components(separatedBy: "\n")
        let kept = lines.filter { line in
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.range(of: #"^\[附图 \d+\] "#, options: .regularExpression) != nil { return false }
            if t == "附图：" { return false }
            if t.range(of: #"^用户发来了 \d+ 张图片，请先查看：$"#, options: .regularExpression) != nil { return false }
            return true
        }
        guard kept.count != lines.count else { return text }
        return kept.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// The stored paths of a goal's or a message's images, from its event data.
    static func paths(_ d: JSON) -> [String] { d["images"].array.map { $0["path"].str }.filter { !$0.isEmpty } }
}
