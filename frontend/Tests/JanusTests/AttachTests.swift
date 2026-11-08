import AppKit
import Testing
@testable import Janus

/// A solid image of the given size, as PNG.
private func png(_ w: Int, _ h: Int, noise: Bool = false) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    if let p = rep.bitmapData {
        var seed: UInt32 = 7
        for i in 0..<(rep.bytesPerRow * h) {
            if noise { seed = seed &* 1_103_515_245 &+ 12345; p[i] = UInt8(truncatingIfNeeded: seed >> 16) } else { p[i] = 200 }
        }
    }
    return rep.representation(using: .png, properties: [:])!
}

/// A pasteboard of its own, so the test never touches the person's clipboard.
private func board() -> NSPasteboard { NSPasteboard(name: .init("janus-test-\(UUID().uuidString)")) }

@Test func imageTypesComeFromTheBytes() {
    #expect(Attach.sniff(png(2, 2)) == "image/png")
    #expect(Attach.sniff(Data([0xFF, 0xD8, 0xFF, 0xE0])) == "image/jpeg")
    #expect(Attach.sniff(Data("GIF89a".utf8)) == "image/gif")
    #expect(Attach.sniff(Data("RIFF\0\0\0\0WEBPVP8 ".utf8)) == "image/webp")
    #expect(Attach.sniff(Data("hello".utf8)) == nil)
}

@Test func smallImagesAreKeptAndLargeOnesScaledDown() throws {
    let small = png(40, 30)
    let kept = try #require(Attach.prepare(small, name: "a.png"))
    #expect(kept.data == small && kept.mimeType == "image/png")

    // Noise does not compress: well over the limit as PNG.
    let big = png(3000, 2000, noise: true)
    #expect(big.count > Attach.maxBytes)
    let scaled = try #require(Attach.prepare(big, name: "截图.png"))
    #expect(scaled.mimeType == "image/jpeg" && scaled.name == "截图.jpg")
    #expect(scaled.data.count <= Attach.maxBytes)
    let rep = try #require(NSBitmapImageRep(data: scaled.data))
    #expect(max(rep.pixelsWide, rep.pixelsHigh) == Int(Attach.maxSide))

    // TIFF (what many apps put on the clipboard) becomes PNG.
    let tiff = try #require(NSImage(data: small)?.tiffRepresentation)
    #expect(Attach.prepare(tiff, name: "粘贴的图片.png")?.mimeType == "image/png")
    #expect(Attach.prepare(Data("x".utf8), name: "x") == nil)
}

@Test func pasteTakesImagesButLeavesTextAlone() throws {
    let shot = board()
    shot.clearContents()
    shot.setData(png(10, 10), forType: .png)
    #expect(Attach.hasImages(shot))
    #expect(Attach.images(from: shot).count == 1)

    // Text with a picture of itself (a spreadsheet, a rich document) pastes as text.
    let cells = board()
    cells.clearContents()
    cells.setString("A1\tB1", forType: .string)
    cells.setData(png(10, 10), forType: .png)
    #expect(!Attach.hasImages(cells))
    #expect(Attach.images(from: cells).isEmpty)

    // Image files copied in Finder are read; any other file pastes as before.
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("janus-attach-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let file = dir.appendingPathComponent("设计稿.png")
    try png(20, 20).write(to: file)
    let finder = board()
    finder.clearContents()
    finder.writeObjects([file as NSURL])
    let got = Attach.images(from: finder)
    #expect(got.count == 1 && got[0].name == "设计稿.png")
    let notes = dir.appendingPathComponent("notes.txt")
    try Data("x".utf8).write(to: notes)
    let other = board()
    other.clearContents()
    other.writeObjects([notes as NSURL])
    #expect(!Attach.hasImages(other) && Attach.images(from: other).isEmpty)
}

@Test func anImageCopiedWithItsLinkStillPastesAsAnImage() {
    let browser = board()
    browser.clearContents()
    browser.setData(png(10, 10), forType: .png)
    browser.setString("https://example.com/picture.png", forType: .string)
    #expect(Attach.hasImages(browser))
    #expect(Attach.images(from: browser).count == 1)
}

/// ⌘V only reaches a text view when Edit > Paste is available, and the standard text view
/// turns Paste off for an image-only clipboard. The composer must keep it on.
@Test @MainActor func anImageOnlyClipboardKeepsCommandVPasteAvailable() {
    let pb = board()
    pb.clearContents()
    pb.setData(png(10, 10), forType: .png)

    let tv = ComposerField.TextView(frame: NSRect(x: 0, y: 0, width: 200, height: 40))
    tv.isEditable = true
    tv.pasteboard = pb
    var got: [PastedImage] = []
    tv.onImages = { got = $0 }

    let item = NSMenuItem(title: "Paste", action: #selector(NSTextView.paste(_:)), keyEquivalent: "v")
    #expect(tv.validateMenuItem(item))
    #expect(tv.validateUserInterfaceItem(item))
    tv.paste(nil)
    #expect(got.count == 1)
}

@Test func theLinesNamingImagesAreHidden() {
    #expect(Attach.visibleText("看看这个报错\n\n附图：\n[附图 1] /h/runs/r/attachments/1.png\n[附图 2] /h/runs/r/attachments/2.png") == "看看这个报错")
    #expect(Attach.visibleText("用户发来了 1 张图片，请先查看：\n[附图 1] /h/runs/r/attachments/1.png") == "")
    #expect(Attach.visibleText("普通消息\n附图在后面") == "普通消息\n附图在后面", "text without image lines is unchanged")
    #expect(Attach.paths(["images": [["path": "/a.png"], ["path": ""]]]) == ["/a.png"])
}

@Test func messagesKeepTheirImagesInTheDerivedState() {
    let path = "/h/runs/r/attachments/1.png"
    let evs = [
        Event(["seq": 1, "ts": 1, "type": "run.created", "actor": "user", "text": "", "data": ["goal": "照着图改", "images": [["path": "/h/runs/r/attachments/0.png"]]]]),
        Event(["seq": 2, "ts": 2, "type": "user.message", "actor": "user", "to": "lead", "text": "", "data": ["id": "m1", "text": JSON("看这里\n\n附图：\n[附图 1] \(path)"), "delivery": "pending", "images": [["path": JSON(path)]]]]),
    ]
    let st = derive(evs)
    #expect(st.run.images == ["/h/runs/r/attachments/0.png"])
    #expect(st.msgs["m1"]?.images == [path])
}
