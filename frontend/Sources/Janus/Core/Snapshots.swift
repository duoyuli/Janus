import AppKit
import SwiftUI

/// Development aid: with `JANUS_SNAPSHOT_DIR` set, the app reads one command per line from
/// `<dir>/cmd` and saves pictures of its own window, so screens can be checked without screen
/// recording rights (docs/operations/testing.md#macos_app).
///
///     open inbox | open run:<id> | open home:<ws> | open settings:<tab> | page board|audit|team|task:<id>|agent:<id>
///     role <id> | agent-edit <id> | close-sheet | doc <path> | size <w>x<h> | shot <name>
///     snooze-first | wake-all | close-window | state <name> (writes <name>.json: inbox and Dock state)
@MainActor
enum Snapshots {
    static func start(_ model: AppModel) {
        guard let dir = ProcessInfo.processInfo.environment["JANUS_SNAPSHOT_DIR"], !dir.isEmpty else { return }
        let cmd = URL(fileURLWithPath: dir).appendingPathComponent("cmd")
        Task { @MainActor in
            while true {
                try? await Task.sleep(nanoseconds: 700_000_000)
                guard let text = try? String(contentsOf: cmd, encoding: .utf8), !text.isEmpty else { continue }
                try? "".write(to: cmd, atomically: true, encoding: .utf8)
                for line in text.split(separator: "\n") {
                    await run(String(line), model: model, dir: dir)
                    try? await Task.sleep(nanoseconds: 900_000_000)
                }
            }
        }
    }

    private static func run(_ line: String, model: AppModel, dir: String) async {
        let parts = line.split(separator: " ", maxSplits: 1).map(String.init)
        guard let store = model.store, parts.count >= 1 else { return }
        let arg = parts.count > 1 ? parts[1] : ""
        let kv = arg.split(separator: ":", maxSplits: 1).map(String.init)
        switch parts[0] {
        case "open":
            switch kv.first {
            case "run": store.open(.run(kv.last ?? ""))
            case "inbox": store.open(.inbox)
            case "home": store.open(.home(kv.last ?? ""))
            case "settings": store.openSettings(kv.last ?? "agents")
            default: break
            }
        case "page":
            guard let r = store.run else { return }
            switch kv.first {
            case "audit": r.page = .audit
            case "team": r.page = .team
            case "task": r.page = .task(kv.last ?? "")
            case "agent": r.page = .agent(kv.last ?? "")
            case "replay": r.replay = Int(kv.last ?? "") ?? 1; r.page = .board
            default: r.page = .board; r.replay = nil
            }
        case "role": store.editingRole = store.role(arg)
        case "workspace": store.editingWorkspace = store.workspace(arg)
        case "close-sheet": store.editingRole = nil; store.editingWorkspace = nil; store.showingSettings = false
        case "snooze-first":
            if let e = store.inbox.open.first(where: \.canWait) { store.inbox.snooze(e.key, until: Date().addingTimeInterval(3600)) }
        case "wake-all": for e in store.inbox.later { store.inbox.wake(e.key) }
        case "close-window": NSApp.windows.first { $0.isVisible && $0.canBecomeMain }?.close()
        case "state":
            // What the inbox and the menu bar show, for checks that a picture cannot make.
            let entries = { (xs: [InboxEntry]) in JSON.array(xs.map { ["key": JSON($0.key), "title": JSON($0.title), "canWait": JSON($0.canWait), "canSettle": JSON($0.canSettle)] }) }
            let j: JSON = [
                "screen": JSON(String(describing: store.screen)), "settings": JSON(store.showingSettings),
                "open": entries(store.inbox.open), "later": entries(store.inbox.later),
                "dock": JSON(NSApp.activationPolicy() == .regular),
                "windows": JSON(NSApp.windows.filter { $0.isVisible && $0.canBecomeMain }.count),
            ]
            try? j.data(pretty: true).write(to: URL(fileURLWithPath: dir).appendingPathComponent("\(arg.isEmpty ? "state" : arg).json"))
        case "doc": ArtifactWindow.open(path: arg, api: store.api)
        case "paste":
            // Paste an image file into the first composer of the key window, through the same
            // handler as ⌘V, from a pasteboard of its own (the person's clipboard is untouched).
            let pb = NSPasteboard(name: .init("janus-snapshot-paste"))
            pb.clearContents()
            pb.writeObjects([URL(fileURLWithPath: arg) as NSURL])
            let wins = NSApp.windows.filter { $0.isVisible && $0.contentView != nil }
            func find(_ v: NSView) -> ComposerField.TextView? { (v as? ComposerField.TextView) ?? v.subviews.lazy.compactMap(find).first }
            if let v = (wins.first(where: { $0.isKeyWindow }) ?? wins.first)?.contentView.flatMap(find) { v.takeImages(from: pb) }
        case "size":
            let wh = arg.split(separator: "x").compactMap { Double($0) }
            if wh.count == 2, let w = NSApp.windows.first(where: { $0.isVisible && $0.contentView != nil }) {
                w.setFrame(NSRect(x: 40, y: 40, width: wh[0], height: wh[1]), display: true)
            }
        case "shot":
            // A sheet is its own window; picture the frontmost one.
            let wins = NSApp.windows.filter { $0.isVisible && $0.contentView != nil }
            guard let w = wins.first(where: { $0.isSheet }) ?? wins.first(where: { $0.isKeyWindow }) ?? wins.first, let v = w.contentView else { return }
            let url = URL(fileURLWithPath: dir).appendingPathComponent("\(arg.isEmpty ? "shot" : arg).png")
            // The window server's picture of our own window is what is really on screen;
            // cacheDisplay is the fallback and loses translucent fills.
            // Without screen recording rights the window server may hand back a blank picture.
            if let cg = windowImage(w.windowNumber), !blank(cg) {
                try? NSBitmapImageRep(cgImage: cg).representation(using: .png, properties: [:])?.write(to: url)
            } else if let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds) {
                v.cacheDisplay(in: v.bounds, to: rep)
                try? rep.representation(using: .png, properties: [:])?.write(to: url)
            }
        default: break
        }
    }

    /// A picture of one flat colour: what the window server returns when it may not show the window.
    private static func blank(_ cg: CGImage) -> Bool {
        let rep = NSBitmapImageRep(cgImage: cg)
        guard rep.pixelsWide > 0, rep.pixelsHigh > 0, let first = rep.colorAt(x: 0, y: 0) else { return true }
        for y in stride(from: 0, to: rep.pixelsHigh, by: max(1, rep.pixelsHigh / 40)) {
            for x in stride(from: 0, to: rep.pixelsWide, by: max(1, rep.pixelsWide / 40)) where rep.colorAt(x: x, y: y) != first { return false }
        }
        return true
    }

    /// CGWindowListCreateImage, looked up at run time: it is deprecated, and this development aid
    /// should not tie the build to an SDK that still declares it.
    private static func windowImage(_ number: Int) -> CGImage? {
        typealias Capture = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
        guard let sym = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") else { return nil }
        let capture = unsafeBitCast(sym, to: Capture.self)
        // .optionIncludingWindow = 1 << 3; .boundsIgnoreFraming = 1 << 0, .bestResolution = 1 << 3
        return capture(.null, 1 << 3, UInt32(number), (1 << 0) | (1 << 3))?.takeRetainedValue()
    }
}
