import AppKit
import SwiftUI

/// Development aid: with `JANUS_SNAPSHOT_DIR` set, the app reads one command per line from
/// `<dir>/cmd` and saves pictures of its own window, so screens can be checked without screen
/// recording rights (docs/operations/20261031-testing.md#macos_app).
///
///     open inbox | open run:<id> | open home:<ws> | open settings:<tab> (library:skill|library:mcp for the library's tabs) | page board|audit|team|task:<id>|agent:<id>
///     role <id> | agent-edit <id>|new | mcp-edit new|<name>|catalog:<pick> [json] | close-sheet | doc <path> | size <w>x<h> | shot <name>
///     snooze-first | wake-all | close-window | state <name> (writes <name>.json: inbox and Dock state)
///     reload (settings, library and runs read again)
///     appearance light|dark|system | review <task> | window <title prefix> (which window `shot` takes)
///     inbox-act <entry>:<action> (clicks a quick answer in the inbox, by position from 0) | density detail|summary | draft <task or agent> <text>
@MainActor
enum Snapshots {
    /// The window `shot` pictures when set: the first visible one whose title starts with it.
    private static var target: String?

    private static let dir = ProcessInfo.processInfo.environment["JANUS_SNAPSHOT_DIR"].flatMap { $0.isEmpty ? nil : $0 }
    /// Driven by a test (`JANUS_SNAPSHOT_DIR` set).
    static var active: Bool { dir != nil }

    static func start(_ model: AppModel) {
        guard let dir else { return }
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
            case "chats": store.open(.chats)
            case "home": store.open(.home(kv.last ?? ""))
            case "settings":
                // Permissions and updates are sections of the general pane, not tabs.
                switch kv.last {
                case "permissions": store.openSettings(.permissions)
                case "updates": store.openSettings(.updates)
                // The library's tabs: `library:skill`, `library:mcp`.
                case let tab? where tab.hasPrefix("library:"):
                    store.openLibrary(AppStore.LibraryKind(rawValue: String(tab.dropFirst("library:".count))) ?? .skill)
                default: store.openSettings(AppStore.SettingsTab(rawValue: kv.last ?? "") ?? .agents)
                }
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
        // Open or close the board's folded parts (`fold usage|progress|risks|tasks|all|T3 open|closed`),
        // which a picture alone cannot reach.
        case "fold":
            guard let r = store.run else { return }
            let p = arg.split(separator: " ", maxSplits: 1).map(String.init)
            let open = p.count < 2 || p[1] != "closed"
            switch p.first {
            case "usage": r.usageOpen = open
            case "progress": r.progressOpen = open
            case "risks": r.risksOpen = open
            case "tasks": for t in r.shown.orderedTasks { r.taskOpen[t.id] = open }
            case "all":
                r.usageOpen = open; r.progressOpen = open; r.risksOpen = open
                for t in r.shown.orderedTasks { r.taskOpen[t.id] = open }
            case let id? where !id.isEmpty: r.taskOpen[id] = open
            default: break
            }
        // Read settings, the library and the run list again, as a fresh launch would: what a test
        // wrote through the API (instead of the app's own editors) shows up.
        case "reload":
            await store.loadSetup(); await store.loadLibrary(); await store.loadRuns(); await store.loadAdapters(quiet: true)
        // The editor is a sheet of the settings panel, as when a person opens it from there.
        case "role": if let r = store.role(arg) { store.editRole(r) }
        case "workspace": store.editingWorkspace = store.workspace(arg)
        // The library's MCP editor: `mcp-edit new|<name>|catalog:<pick> [json]`.
        case "mcp-edit":
            let p = arg.split(separator: " ").map(String.init)
            store.openLibrary(.mcp)
            if let pick = Catalog.servers.first(where: { "catalog:\($0.name)" == p.first }) {
                store.editingMcp = AppStore.McpEditing(draft: pick.draft, item: nil, json: p.last == "json", note: pick.needs)
            } else {
                await store.editMcp(store.libraryItems.first { $0["kind"].str == "mcp" && $0["name"].str == p.first }, json: p.last == "json")
            }
        // The agent editor: `agent-edit <id>`, or `agent-edit new`.
        case "agent-edit": store.editAgent(arg == "new" ? "" : arg)
        case "close-sheet":
            store.closeRoleEditor(); store.editingWorkspace = nil; store.editingMcp = nil; store.showingSettings = false
            model.permissions.dismissReview()
        case "snooze-first":
            if let e = store.inbox.open.first(where: \.canWait) { store.inbox.snooze(e.key, until: Date().addingTimeInterval(3600)) }
        case "wake-all": for e in store.inbox.later { store.inbox.wake(e.key) }
        case "close-window": NSApp.windows.first { $0.isVisible && $0.canBecomeMain }?.close()
        // Look at both appearances without changing the system's.
        case "appearance": NSApp.appearance = arg == "light" ? NSAppearance(named: .aqua) : arg == "dark" ? NSAppearance(named: .darkAqua) : nil
        case "state":
            // What the app shows, for checks that a picture cannot make: the inbox, the menu bar,
            // the settings it loaded, and what it derived for the open run (the end-to-end test
            // compares these with the server, docs/operations/20261031-testing.md#product_e2e).
            let entries = { (xs: [InboxEntry]) in
                JSON.array(xs.map { ["key": JSON($0.key), "run": JSON($0.run), "kind": JSON($0.kind), "title": JSON($0.title),
                                     "canWait": JSON($0.canWait), "canSettle": JSON($0.canSettle), "actions": JSON($0.actions.map(\.label))] })
            }
            let ids = { (xs: [JSON]) in JSON.array(xs.map { ["id": $0["id"], "phase": $0["phase"], "status": $0["status"]] }) }
            var j: JSON = [
                "screen": JSON(String(describing: store.screen)), "settings": JSON(store.showingSettings),
                "settingsTab": JSON(store.settingsTab.rawValue), "permissionsReview": JSON(model.permissions.reviewPresented),
                "open": entries(store.inbox.open), "later": entries(store.inbox.later),
                "dock": JSON(NSApp.activationPolicy() == .regular),
                "windows": JSON(NSApp.windows.filter { $0.isVisible && $0.canBecomeMain }.count),
                "connections": ids(store.connections), "roles": ids(store.roles), "workspaces": ids(store.workspaces), "runs": ids(store.runs),
                "chats": ids(store.chats),
                // The task records each workspace page read and shows, by workspace.
                "histories": .object(store.histories.mapValues { ids($0.runs) }),
                "adapterUpdates": JSON.array(store.adapterUpdates.map { $0["id"] }),
            ]
            if let r = store.run {
                let s = r.state
                j["run"] = [
                    "id": JSON(r.id), "loaded": JSON(r.loaded), "live": JSON(r.live), "page": JSON(String(describing: r.page)),
                    "chat": JSON(r.isChat),
                    "status": JSON(s.run.status.rawValue), "paused": JSON(s.run.paused), "delivered": JSON(s.result != nil),
                    "accepted": JSON(s.result?.accepted != nil), "events": JSON(s.events.count),
                    "tasks": JSON.array(s.orderedTasks.map { t in
                        ["id": JSON(t.id), "status": JSON(t.status.rawValue), "agent": JSON(t.agent), "attempts": JSON(t.attempts.count),
                         "verdict": JSON(t.verdict?.rawValue), "question": JSON(t.question?.question),
                         "git": JSON(SessionView.taskGitNote(s, t.id))]
                    }),
                    "inbox": JSON.array(s.inbox.map { ["kind": JSON($0.kind), "key": JSON($0.key), "task": JSON($0.task)] }),
                    "agents": .object(s.agents.mapValues { JSON($0.phase) }),
                    "tokens": r.usage.run.tokens.total.map { JSON($0) } ?? .null,
                    // The run card's wall-clock time right now, from the same displayed ledger and
                    // live judgement as the view: the end-to-end test checks it advances by the
                    // second without new events (docs/operations/20261031-testing.md#product_e2e).
                    "elapsedMs": r.usageDisplayed(at: L.nowMs()).run.durationMs.map { JSON($0) } ?? .null,
                    // Where the run works and where its accepted result went, as the delivery shows it.
                    "worktree": JSON(s.run.repo.isEmpty ? nil : s.run.cwd), "branch": JSON(s.run.branch.isEmpty ? nil : s.run.branch), "merged": .bool(s.run.merged),
                ]
            }
            try? j.data(pretty: true).write(to: URL(fileURLWithPath: dir).appendingPathComponent("\(arg.isEmpty ? "state" : arg).json"))
        case "doc": ArtifactWindow.open(path: arg, api: store.api)
        case "window": target = arg.isEmpty ? nil : arg
        case "density": UserDefaults.janus.set(arg, forKey: "janus.chatDensity")
        // Type into a session's composer: `draft <task or agent> <text>`.
        case "draft":
            let p = arg.split(separator: " ", maxSplits: 1).map(String.init)
            if let run = store.run, let who = p.first { run.sessionDrafts["sess-" + who] = p.count > 1 ? p[1] : "" }
        case "inbox-act":
            let n = kv.compactMap { Int($0) }
            let open = store.inbox.open
            if n.count == 2, n[0] < open.count, n[1] < open[n[0]].actions.count { await store.inbox.act(open[n[0]], open[n[0]].actions[n[1]], store: store) }
        // The review window for a task's latest diff, in the open run.
        case "review":
            if let run = store.run, let a = run.state.artifacts.last(where: { $0.task == arg && $0.kind == "git_diff" }) { DiffWindow.open(run: run, artifact: a) }
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
            let named = target.flatMap { t in wins.first { $0.title.hasPrefix(t) } }
            guard let w = named ?? wins.first(where: { $0.isSheet }) ?? wins.first(where: { $0.isKeyWindow })
                ?? wins.first(where: { $0.canBecomeMain }) ?? wins.first, let v = w.contentView else { return }
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
        // Window numbers are Ints but the API takes a UInt32; a menu bar or status window can be
        // outside that range, and the conversion must not trap the app.
        guard let id = UInt32(exactly: number),
              let sym = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") else { return nil }
        let capture = unsafeBitCast(sym, to: Capture.self)
        // .optionIncludingWindow = 1 << 3; .boundsIgnoreFraming = 1 << 0, .bestResolution = 1 << 3
        return capture(.null, 1 << 3, id, (1 << 0) | (1 << 3))?.takeRetainedValue()
    }
}
