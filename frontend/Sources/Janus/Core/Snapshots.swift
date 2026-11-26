import AppKit
import SwiftUI

/// Development aid: with `JANUS_SNAPSHOT_DIR` set, the app reads one command per line from
/// `<dir>/cmd` and saves pictures of its own window, so screens can be checked without screen
/// recording rights (docs/operations/20261031-testing.md#macos_app).
///
///     open inbox | open run:<id> | open home:<ws> | open settings:<tab> (library:skill|library:mcp for the library's tabs; permissions|updates|host-env for a section of 通用) | page board|audit|team|task:<id>|agent:<id>
///     workspace <id> | brief <id> (its settings, or its shared material) | loop-new <id> | host-hide <path> on|off | host-defaults | host-try <command>
///     role <id> | agent-edit <id>|new | mcp-edit new|<name>|catalog:<pick> [json] | close-sheet | doc <path> | size <w>x<h> | shot <name>
///     snooze-first | wake-all | ignore <entry key> | close-window | state <name> (writes <name>.json: inbox and Dock state)
///     reload (settings, library and runs read again)
///     appearance light|dark|system | review <task> | window <title prefix> (which window `shot` takes)
///     inbox-act <entry key>:<action> (clicks a quick answer of that inbox entry, actions from 0) | density detail|summary | draft task:<id>|agent:<id> <text>
///     send-key commandReturn|return | press return|shift-return|command-return (into the first composer) | update-channel stable|rc
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
                try? await Task.sleep(nanoseconds: 100_000_000)
                guard let text = try? String(contentsOf: cmd, encoding: .utf8), !text.isEmpty else { continue }
                try? "".write(to: cmd, atomically: true, encoding: .utf8)
                let lines = text.split(separator: "\n").map(String.init)
                for (i, line) in lines.enumerated() {
                    await run(line, model: model, dir: dir)
                    try? await Task.sleep(nanoseconds: settle(after: line, before: i + 1 < lines.count ? lines[i + 1] : nil))
                }
            }
        }
    }

    /// How long the app settles between two commands of a batch: a picture waits for the page to
    /// draw what the commands before it opened; anything else only lets the views update. Writing
    /// the state ends a batch, so nothing waits after it.
    static func settle(after line: String, before next: String?) -> UInt64 {
        guard let next, !line.hasPrefix("state") else { return 0 }
        return next.hasPrefix("shot") ? 900_000_000 : 200_000_000
    }

    private static func run(_ line: String, model: AppModel, dir: String) async {
        let parts = line.split(separator: " ", maxSplits: 1).map(String.init)
        guard parts.count >= 1 else { return }
        guard let store = model.store else {
            // Not connected to a server yet (an app still starting its own): only `state` answers,
            // so a test waiting for the connection keeps asking instead of waiting on a batch that
            // nothing answers.
            if parts[0] == "state" {
                let j: JSON = ["connections": .null]
                try? j.data(pretty: true).write(to: URL(fileURLWithPath: dir).appendingPathComponent("\(parts.count > 1 ? parts[1] : "state").json"))
            }
            return
        }
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
                case "host-env": store.openSettings(.hostEnv)
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
        // Open or close the board's folded parts (`fold usage|progress|review|tasks|all|T3 open|closed`),
        // which a picture alone cannot reach.
        case "fold":
            guard let r = store.run else { return }
            let p = arg.split(separator: " ", maxSplits: 1).map(String.init)
            let open = p.count < 2 || p[1] != "closed"
            switch p.first {
            case "usage": r.usageOpen = open
            case "progress": r.progressOpen = open
            case "review": r.reviewOpen = open
            case "tasks": for t in r.shown.orderedTasks { r.taskOpen[t.id] = open }
            case "all":
                r.usageOpen = open; r.progressOpen = open; r.reviewOpen = open
                for t in r.shown.orderedTasks { r.taskOpen[t.id] = open }
            case let id? where !id.isEmpty: r.taskOpen[id] = open
            default: break
            }
        // Read settings, the library and the run list again, as a fresh launch would: what a test
        // wrote through the API (instead of the app's own editors) shows up.
        case "reload":
            await store.loadSetup(); await store.loadLibrary(); await store.loadLoops(); await store.loadRuns(); await store.loadAdapters(quiet: true)
        // The editor is a sheet of the settings panel, as when a person opens it from there.
        case "role": if arg == "new" { store.newRole() } else if let r = store.role(arg) { store.editRole(r.json) }
        case "workspace": store.editingWorkspace = store.workspace(arg)?.json
        // 本机环境 as the person changes it: `host-hide <path> on|off`, `host-defaults`, `host-try <command>`.
        case "host-hide":
            let p = arg.split(separator: " ").map(String.init)
            if store.hostEnv == nil { _ = await store.loadHostEnv() }
            if let path = p.first { await store.hideFromAgents(HostEnv.with(store.hostHidden, path, hide: p.last == "on")) }
        case "host-defaults": await store.hideFromAgents(nil)
        case "host-try": _ = await store.tryOnHost(arg)
        // Workspace `arg`'s shared material, in its own sheet.
        case "brief": store.editingBrief = store.workspace(arg)?.json
        // The form for a new Loop in workspace `arg`.
        case "loop-new": store.editingLoop = AppStore.blankLoop(arg)
        // A note of a workspace's team memory, in its sheet: `memory <id>` (its page must have shown).
        case "memory": store.editingMemory = store.memories.values.flatMap { $0["memories"].array }.first { $0["id"].str == arg }
        // Consolidate a workspace's team memory now, as the button on its page does.
        case "consolidate": await store.consolidate(arg)
        // The library's MCP editor: `mcp-edit new|<name>|catalog:<pick> [json]`.
        case "mcp-edit":
            let p = arg.split(separator: " ").map(String.init)
            store.openLibrary(.mcp)
            if let pick = Catalog.servers.first(where: { "catalog:\($0.name)" == p.first }) {
                store.editingMcp = AppStore.McpEditing(draft: pick.draft, item: nil, json: p.last == "json", note: pick.needs)
            } else {
                await store.editMcp(store.libraryItems.first { $0["kind"].str == "mcp" && $0["name"].str == p.first }, json: p.last == "json")
            }
        // The agent editor: `agent-edit <id>` or `agent-edit new [kind auth [template]]`.
        case "agent-edit":
            let args = arg.split(separator: " ").map(String.init)
            if args.first == "new" {
                store.editAgent(.new(kind: args.dropFirst().first ?? "codex", auth: args.dropFirst(2).first, template: args.dropFirst(3).first == "template"))
            } else { store.editAgent(.existing(arg)) }
        // A digital employee's picture from an image file, cropped and saved the way the editor
        // does: `avatar <role> <path>`, or `avatar <role> none` to take it back to the initials.
        case "avatar":
            let p = arg.split(separator: " ", maxSplits: 1).map(String.init)
            if p.count == 2 {
                let png = p[1] == "none" ? nil : (try? Data(contentsOf: URL(fileURLWithPath: p[1]))).flatMap { AvatarImages.square($0) }
                if p[1] == "none" || png != nil { try? await store.setAvatar(p[0], png: png) }
            }
        // The open chat's proposal of digital employees, driven as the person does:
        // `proposal-open` opens the sheet, `proposal-toggle <n>` ticks or unticks one,
        // `proposal-agent <id>` picks the agent, `proposal-create` creates the ticked ones,
        // `proposal-decline` creates none.
        case "proposal-open": if let r = store.run { await store.openProposal(r) }
        case "proposal-toggle":
            if store.proposalSheet?.picked.contains(arg) == true { store.proposalSheet?.picked.remove(arg) } else { store.proposalSheet?.picked.insert(arg) }
        case "proposal-agent": store.proposalSheet?.connection = arg
        case "proposal-create": await store.createProposed()
        case "proposal-decline": if let r = store.run { await store.declineProposal(r) }
        // A group's menu: `group-bind <group>|<agent>`, `group-rename <from>|<to>`.
        case "group-bind":
            let p = arg.components(separatedBy: "|")
            if p.count == 2 { await store.bindRoleGroup(p[0], connection: p[1]) }
        case "group-rename":
            let p = arg.components(separatedBy: "|")
            if p.count == 2 { await store.renameRoleGroup(p[0], to: p[1]) }
        // Settings › 数字员工 › 找招聘顾问….
        case "recruiter-chat": await store.chatWithRecruiter()
        case "close-sheet":
            store.closeRoleEditor(); store.proposalSheet = nil; store.editingWorkspace = nil; store.editingBrief = nil; store.editingLoop = nil; store.editingMemory = nil; store.editingMcp = nil; store.showingSettings = false
            model.permissions.dismissReview()
        case "snooze-first":
            if let e = store.inbox.open.first(where: \.canWait) { store.inbox.snooze(e.key, until: Date().addingTimeInterval(3600)) }
        case "wake-all": for e in store.inbox.later { store.inbox.wake(e.key) }
        case "ignore": store.inbox.ignore(arg)
        case "close-window": NSApp.windows.first { $0.isVisible && $0.canBecomeMain }?.close()
        // Look at both appearances without changing the system's.
        case "appearance": NSApp.appearance = arg == "light" ? NSAppearance(named: .aqua) : arg == "dark" ? NSAppearance(named: .darkAqua) : nil
        case "state":
            // What the app shows, for checks that a picture cannot make: the inbox, the menu bar,
            // the settings it loaded, and what it derived for the open run (the end-to-end test
            // compares these with the server, docs/operations/20261031-testing.md#product_e2e).
            let entries = { (xs: [InboxEntry]) in
                JSON.array(xs.map { ["key": JSON($0.key), "run": JSON($0.run), "kind": JSON($0.kind), "title": JSON($0.title),
                                     "canWait": JSON($0.canWait), "actions": JSON($0.actions.map(\.label))] })
            }
            let ids = { (xs: [JSON]) in JSON.array(xs.map { ["id": $0["id"], "phase": $0["phase"], "status": $0["status"], "label": JSON(runLabel($0).0), "origin": $0["origin"], "loopId": $0["loopId"]] }) }
            var j: JSON = [
                "screen": JSON(String(describing: store.screen)), "settings": JSON(store.showingSettings),
                "settingsTab": JSON(store.settingsTab.rawValue),
                // Which sheet of a workspace is open: its settings or its shared material.
                "editingWorkspace": store.editingWorkspace?["id"] ?? .null, "editingBrief": store.editingBrief?["id"] ?? .null,
                // 本机环境 as the settings page shows it: tools found, others found, what is shared.
                "hostEnv": store.hostEnv.map { ["hidden": JSON.array(store.hostHidden.map(JSON.init)), "defaults": $0["defaults"], "janusHome": $0["janusHome"]] } ?? .null,
                "hostTry": store.hostTry ?? .null, "permissionsReview": JSON(model.permissions.reviewPresented),
                "open": entries(store.inbox.open), "later": entries(store.inbox.later),
                "dock": JSON(NSApp.activationPolicy() == .regular),
                "windows": JSON(NSApp.windows.filter { $0.isVisible && $0.canBecomeMain }.count),
                "connections": ids(store.connections), "roles": ids(store.roles.map(\.json)), "workspaces": ids(store.workspaces.map(\.json)), "runs": ids(store.runs),
                "claudeEnvironments": .object(Dictionary(uniqueKeysWithValues: store.connections.filter {
                    store.envAuth($0["kind"].str, $0["auth"].str)
                }.map { ($0["id"].str, JSON(Setup.envText($0["env"]))) })),
                // The open agent editor's environment text, as the person sees it.
                "agentEditorEnv": store.agentEditorEnv.map(JSON.init) ?? .null,
                "chats": ids(store.chats),
                // Every workspace's Loops as the app shows them: what each is doing and how it reads.
                "loops": JSON.array(store.loops.map {
                    ["id": $0["id"], "run": $0["run"], "workspace": $0["workspace"], "name": $0["name"], "phase": $0["status"]["phase"],
                     "wakes": $0["status"]["wakes"], "label": JSON(LoopText.phase($0).0),
                     "schedule": JSON(LoopText.schedule($0)), "asked": LoopText.asked($0).map(JSON.init) ?? .null]
                }),
                "sendKey": JSON(SendKeySetting.shared.value.rawValue),
                // Settings › 通用 › 更新: whether updates install themselves.
                "autoUpdate": JSON(Updater.shared.automatic),
                "updateChannel": JSON(Updater.shared.channel.rawValue),
                // Whether this app would update itself: never while a test drives it.
                "updateEnabled": JSON(Updater.shared.enabled),
                // Settings › 数字员工 as listed: each group's title and its employees, in order.
                // The digital employee being edited, and whether its editor offers 招聘顾问 (a new one).
                "editingRole": store.editingRole.map { ["id": $0["id"], "offersRecruiter": JSON(store.offersRecruiter($0))] } ?? .null,
                // The new-chat page and the workspace editor list them the same way.
                "roleGroups": JSON.array(store.roleSections.map { section in
                    ["group": JSON(section.group), "title": JSON(AppStore.groupTitle(section.group)), "roles": JSON.array(section.roles.map { JSON($0.id) })]
                }),
                // The proposal sheet as shown: whose chat, each employee (number, name, group, lead,
                // whether the name is taken) and which are ticked.
                "proposalSheet": store.proposalSheet.map { p in
                    ["chat": JSON(p.chat), "error": p.error.map(JSON.init) ?? .null, "connection": JSON(p.connection),
                     "picked": JSON.array(p.picked.sorted().map(JSON.init)),
                     "roles": JSON.array(p.roles.map { ["n": $0["n"], "name": $0["name"], "group": $0["group"], "lead": $0["lead"], "existing": $0["existing"]] })]
                } ?? .null,
                // The digital employees whose own picture the app has loaded, with its size in pixels.
                "avatars": .object(AvatarImages.shared.images.mapValues { img in
                    JSON(img.representations.first.map { $0.pixelsWide } ?? 0)
                }),
                // Each workspace's team memory as its page shows it: the notes (kind, title, the
                // line under them) and the consolidation, by workspace.
                "memories": .object(store.memories.mapValues { m in
                    ["notes": JSON.array(m["memories"].array.map {
                        ["id": $0["id"], "kind": JSON(MemoryText.kind($0["kind"].str)), "title": $0["title"], "hook": $0["hook"], "source": JSON(MemoryText.source($0))]
                     }),
                     "running": m["consolidation"]["running"], "last": m["consolidation"]["last"]["run"]]
                }),
                "editingMemory": store.editingMemory?["id"] ?? .null,
                // The task records each workspace page read, by workspace.
                "histories": .object(store.histories.mapValues { ids($0.runs) }),
                // Each workspace's count in the sidebar (its goals' and its Loops' open items).
                "sidebar": .object(Dictionary(uniqueKeysWithValues: store.workspaces.map { w in
                    (w.id, JSON(store.waiting(in: w).reduce(0) { $0 + $1.entries.count }))
                })),
                // The open workspace page as shown: what waits on the person, one row per goal or
                // Loop, and the task records listed below it (those waiting are not listed again).
                "home": {
                    guard case .home(let id) = store.screen, let w = store.workspace(id) else { return .null }
                    let waiting = store.waiting(in: w)
                    let hidden = Set(waiting.map(\.id))
                    let knowledge = HomeView.knowledge(w, store: store)
                    return ["waiting": JSON.array(waiting.map { ["run": JSON($0.id), "loop": $0.loop?["id"] ?? .null, "label": JSON($0.label), "items": JSON($0.entries.count)] }),
                            "records": JSON.array((store.histories[id]?.runs ?? []).filter { !hidden.contains($0["id"].str) }.map { $0["id"] }),
                            // What the two tiles beside the name say, the Loops in the order of their
                            // column, and whether the goal box has the keyboard.
                            "brief": JSON(knowledge.brief), "memory": JSON(knowledge.memory),
                            "loops": JSON.array(LoopText.byAttention(store.loops(of: id)).map { $0["id"] }),
                            "goalFocused": JSON(NSApp.windows.contains { $0.firstResponder is ComposerField.TextView && $0.isVisible && $0.canBecomeMain })]
                }(),
                // Each Agent card's runtime line as the settings page shows it
                // (docs/architecture/20261031-adapters.md#api_shape): what the server says, the
                // state word the line derives, what its version menu holds, and — for a failed
                // install — exactly the two texts the popover shows (the server-worded reason and
                // Janus's own recent steps), so a test can check neither names the install tool.
                "adapters": JSON.array(store.adapters["adapters"].array.map { a in
                    ["id": a["id"], "current": a["current"], "versions": JSON.array(a["versions"].array.map { $0["version"] }),
                     "pinned": a["pinned"], "latest": a["latest"], "job": a["job"],
                     "state": JSON(String(describing: AdapterText.state(a, now: L.nowMs(), seen: store.autoUpdateSeen))),
                     "menuShown": JSON(AdapterText.showsMenu(a)),
                     "menu": JSON.array(AdapterText.menuItems(a).map { item -> JSON in
                         switch item {
                         case .resume(let v): return JSON("resume:\(v)")
                         case .switchTo(let v): return JSON("switch:\(v)")
                         }
                     }),
                     "reason": a["job"]["detail"], "recent": JSON.array(AdapterText.recentNotes(a).map(JSON.init))]
                }),
                // The library's MCP tab as shown: the code-mode switch.
                "mcp": ["codeMode": JSON(store.codeMode)],
                // The library's skill tab as shown: the router guide's catalog mode and order, the
                // groups by where the skills came from, and each skill's usage line and hints.
                "skills": [
                    "searched": JSON(Library.searched(store.library)),
                    "groups": JSON.array(Library.skillGroups(store.libraryItems.filter { $0["kind"].str == "skill" }, sources: store.librarySources).map {
                        ["key": JSON(LibraryView.key($0.origin)), "items": JSON.array($0.items.map { $0["name"] })]
                    }),
                    "items": JSON.array(Library.catalogOrder(store.libraryItems.filter { $0["kind"].str == "skill" }).map {
                        ["name": $0["name"], "sessions": $0["uses"]["sessions"], "usage": JSON(Library.usage($0, now: L.nowMs())),
                         "hints": JSON.array($0["hints"].array.map { $0["kind"] }), "hintText": JSON.array($0["hints"].array.map { JSON(Library.hint($0)) }),
                         "warning": JSON(LibraryView.warning($0))]
                    }),
                ],
            ]
            if let r = store.run {
                let s = r.state
                j["run"] = [
                    "id": JSON(r.id), "loaded": JSON(r.loaded), "live": JSON(r.live), "readOnly": .bool(r.readOnly), "page": JSON(String(describing: r.page)),
                    "chat": JSON(r.isChat), "dream": JSON(r.isDream),
                    // A chat's open proposal of digital employees, as the server gave it.
                    "proposal": r.proposal ?? .null,
                    // Whether the chat shows a proposal at all: `proposal` reads null both for none and
                    // for a proposal that is `JSON.null`.
                    "proposalShown": .bool(r.proposal != nil),
                    // The inspector's 交付物 under 产物 (the 文档 menu's deliverables).
                    "deliverables": JSON.array(r.documents.filter { $0.source == .deliverable }.map { JSON($0.path) }),
                    // A consolidation's outcome as its page shows it.
                    "consolidated": s.events.last { $0.type == .memoryConsolidated }.map { $0.data } ?? .null,
                    // A Loop's page as shown: its Loop, the count in its header and the dispatched runs listed.
                    "loop": r.isLoop ? {
                        let l = store.loop(ofRun: r.id) ?? .null
                        return ["id": l["id"], "title": l["title"], "phase": l["status"]["phase"], "state": l["state"],
                                "dispatchedCount": JSON(LoopText.dispatchedCount(l)),
                                "dispatched": JSON.array(r.loopDispatched.map { ["id": $0["id"], "key": $0["dispatchKey"], "status": $0["status"]] }),
                                // Where each wake starts and ends in its session, with the lines drawn there.
                                "marks": JSON.array(chatSession(s, live: r.live).evs.filter { [.loopWoke, .loopSlept].contains($0.type) }.map { e in
                                    let woke = LoopText.wake(before: e, in: s.events)
                                    return ["seq": JSON(e.seq), "type": JSON(e.type.rawValue),
                                            "text": JSON(e.type == .loopWoke ? LoopText.woke(e) : LoopText.slept(e, woke: woke)),
                                            "tookMs": e.type == .loopSlept ? woke.map { JSON(floatLiteral: e.ts - $0.ts) } ?? .null : .null]
                                })]
                    }() : .null,
                    // What each session's composer holds, by session (`task:T1`, `agent:lead`).
                    "drafts": .object(Dictionary(uniqueKeysWithValues: r.sessionDrafts.map { ($0.key.label, JSON($0.value)) })),
                    // Each task's conversation and the lead's, as their cards lay them out: the rows in
                    // turns at the display level chosen, what the composer suggests for its draft and the
                    // live preview (docs/architecture/20261031-frontend.md#session_turns).
                    "conversations": .object(Dictionary(uniqueKeysWithValues: (s.orderedTasks.map { (SessionTarget.task($0.id), taskSession(s, $0)) } + [(SessionTarget.agent("lead"), agentSession(s, "lead"))]).map { target, info in
                        let detail = UserDefaults.janus.string(forKey: "janus.chatDensity") == ChatDensity.detail.rawValue
                        let draft = r.sessionDrafts[target] ?? ""
                        let live: ChatLive.Session? = { if case .task(let t) = target { return r.preview.of(task: t, agent: nil) } else { return r.preview.of(task: nil, agent: "lead") } }()
                        return (target.label, [
                            "rows": JSON.array(ChatCard.rows(run: r, st: s, info: info, target: target, detail: detail, live: r.canAct).map(Self.row)),
                            "suggestions": JSON.array(Suggestions.rows(SessionView.asking(draft), commands: SessionView.commands(info.evs), files: []).map { ["label": JSON($0.label), "group": $0.group.map(JSON.init) ?? .null] }),
                            "ghost": SessionView.argumentHint(draft, SessionView.commands(info.evs)).map(JSON.init) ?? .null,
                            "preview": ["text": JSON(live?.text ?? ""), "tools": JSON(live?.tools.count ?? 0)],
                        ] as JSON)
                    })),
                    "status": JSON(s.run.status.rawValue), "paused": JSON(s.run.paused), "delivered": JSON(s.result != nil),
                    "goal": JSON(s.run.goal), "queuePosition": r.queuePosition.map { JSON($0) } ?? .null,
                    "accepted": JSON(s.result?.accepted != nil), "events": JSON(s.events.count),
                    // The delivery under review as its card shows it (docs/architecture/20261031-frontend.md#review).
                    "review": s.result.map { res -> JSON in
                        let d = Delivery(res)
                        let strings = { (xs: [String]) in JSON.array(xs.map { JSON($0) }) }
                        let stage = reviewStage(s, live: r.live && !r.isReplaying)
                        return ["stage": JSON(stage.map { String(describing: $0) }), "actions": .bool(stage.map(r.offersReview) ?? false), "n": JSON(d.n),
                                "headline": JSON(d.headline), "decisions": strings(d.decisions), "merge": JSON(d.merge?.rawValue),
                                "changed": strings(d.changed), "requests": strings(roundRequests(s, res).map(\.text)),
                                "signals": JSON.array(reviewSignals(d).map { ["source": JSON(String(describing: $0.source)), "text": JSON($0.text)] }),
                                "tasks": strings(d.acceptance.map(\.task)), "facts": JSON(ReviewBody.factsLine(d.acceptance, changed: d.changed.count))]
                    } ?? .null,
                    "tasks": JSON.array(s.orderedTasks.map { t in
                        ["id": JSON(t.id), "status": JSON(t.status.rawValue), "agent": JSON(t.agent), "attempts": JSON(t.attempts.count),
                         "verdict": JSON(t.verdict?.rawValue), "question": JSON(t.question?.question),
                         "git": JSON(SessionView.taskGitNote(s, t.id))]
                    }),
                    "inbox": JSON.array(s.inbox.map { ["kind": JSON($0.kind), "key": JSON($0.key), "task": JSON($0.task)] }),
                    "agents": .object(s.agents.mapValues { JSON($0.phase.rawValue) }),
                    "tokens": r.usage.run.tokens.total.map { JSON($0) } ?? .null,
                    // The collaborators' time as the usage section words it, and the numbers behind it.
                    "subtaskTime": JSON(r.usage.time.value),
                    "subtaskMs": ["work": JSON(r.usage.time.workMs), "verify": JSON(r.usage.time.verifyMs), "rework": JSON(r.usage.time.reworkMs)],
                    // The run card's wall-clock time right now, from the same displayed ledger and
                    // live judgement as the view: the end-to-end test checks it advances by the
                    // second without new events (docs/operations/20261031-testing.md#product_e2e).
                    "elapsedMs": r.usageDisplayed(at: L.nowMs()).run.durationMs.map { JSON($0) } ?? .null,
                    // Where the run works and where its accepted result went, as the delivery shows it.
                    "worktree": JSON(s.run.repo.isEmpty ? nil : s.run.cwd), "branch": JSON(s.run.branch.isEmpty ? nil : s.run.branch), "merged": .bool(s.run.merged),
                    // The blocks the last agent message is drawn as (`MarkdownText`).
                    "lastReply": JSON.array(MD.blocks(s.events.last { $0.type == .agentMessage }?.data["text"].str ?? "").map { JSON(block($0)) }),
                    "conversation": ChatProbe.json,
                ]
            }
            try? j.data(pretty: true).write(to: URL(fileURLWithPath: dir).appendingPathComponent("\(arg.isEmpty ? "state" : arg).json"))
        case "doc": ArtifactWindow.open(path: arg, api: store.api)
        case "window": target = arg.isEmpty ? nil : arg
        case "density": UserDefaults.janus.set(arg, forKey: "janus.chatDensity")
        // The send key, as Settings › 通用 saves it.
        case "send-key": SendKeySetting.shared.value = SendKey(rawValue: arg) ?? .standard
        // The automatic update switch, as Settings › 通用 › 更新 saves it.
        case "auto-update": Updater.shared.automatic = arg != "off"
        case "update-channel": Updater.shared.channel = Updater.Channel(rawValue: arg) ?? .stable
        // The MCP tab's code-mode switch, as the library saves it.
        case "code-mode": await store.setCodeMode(arg != "off")
        // A key press in the first composer of the key window, through the same handler as typing.
        case "press":
            let flags: NSEvent.ModifierFlags = arg == "shift-return" ? .shift : arg == "command-return" ? .command : []
            if let v = composer(), let e = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0, windowNumber: v.window?.windowNumber ?? 0,
                                                             context: nil, characters: "\r", charactersIgnoringModifiers: "\r", isARepeat: false, keyCode: 36) {
                v.window?.makeFirstResponder(v)
                v.keyDown(with: e)
            }
        // Type into a session's composer: `draft task:<id>|agent:<id> <text>`.
        case "draft":
            let p = arg.split(separator: " ", maxSplits: 1).map(String.init)
            if let run = store.run, let target = p.first.flatMap(SessionTarget.init(label:)) { run.sessionDrafts[target] = p.count > 1 ? p[1] : "" }
        // By the entry's key, not its place: other runs come and go in the inbox meanwhile.
        case "inbox-act":
            if kv.count == 2, let n = Int(kv[1]), let e = store.inbox.open.first(where: { $0.key == kv[0] }), n < e.actions.count {
                await store.inbox.act(e, e.actions[n], store: store)
            }
        // The review window for a task's latest diff, in the open run.
        case "review":
            if let run = store.run, let a = run.state.artifacts.last(where: { $0.task == arg && $0.kind == "git_diff" }) { DiffWindow.open(run: run, artifact: a) }
        case "paste":
            // Paste an image file into the first composer of the key window, through the same
            // handler as ⌘V, from a pasteboard of its own (the person's clipboard is untouched).
            let pb = NSPasteboard(name: .init("janus-snapshot-paste"))
            pb.clearContents()
            pb.writeObjects([URL(fileURLWithPath: arg) as NSURL])
            composer()?.takeImages(from: pb)
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

    /// The first composer of the key window (or the first visible one).
    /// A block as the state names it: its kind, and for a table its header, for code its language.
    /// One conversation row as the test reads it: its kind, and what it shows in words.
    static func row(_ r: ChatTurns.Row) -> JSON {
        switch r {
        case .section(let e, let injected): return ["kind": "section", "type": JSON(e.type.rawValue), "injected": JSON.array(injected.map { JSON($0.data["channel"].string ?? $0.type.rawValue) })]
        case .item(let e): return ["kind": "item", "type": JSON(e.type.rawValue), "seq": JSON(e.seq)]
        case .head(let t, let open):
            return ["kind": "head", "agent": JSON(t.agent), "open": open.map { JSON.bool($0) } ?? .null, "running": JSON(t.running), "stats": JSON(t.stats.text)]
        case .activity(_, let es, let open): return ["kind": "activity", "calls": JSON(es.count), "open": open.map { JSON.bool($0) } ?? .null]
        case .text(let es, let reply): return ["kind": reply ? "reply" : "text", "text": JSON(es.map { $0.data["text"].str }.joined())]
        case .edits(_, let files): return ["kind": "edits", "files": JSON.array(files.map { ["path": JSON($0.path), "added": JSON($0.added), "removed": JSON($0.removed)] })]
        }
    }

    private static func block(_ n: MDDoc.Node) -> String {
        switch n {
        case .heading(let level, _): return "heading\(level)"
        case .para: return "para"
        case .list(let ordered, _, _): return ordered ? "ordered" : "list"
        case .code(let lang, _): return "code:" + lang
        case .quote: return "quote"
        case .table(let header, _, _): return "table:" + header.joined(separator: ",")
        case .rule: return "rule"
        case .image: return "image"
        }
    }

    /// The first composer of the key window, or of another visible window when the key one has
    /// none: scenarios share the app, and a sheet another one left open (a note, the shared
    /// material) is the key window then.
    private static func composer() -> ComposerField.TextView? {
        let wins = NSApp.windows.filter { $0.isVisible && $0.contentView != nil }
        func find(_ v: NSView) -> ComposerField.TextView? { (v as? ComposerField.TextView) ?? v.subviews.lazy.compactMap(find).first }
        return (wins.filter(\.isKeyWindow) + wins.filter { !$0.isKeyWindow }).lazy.compactMap { $0.contentView.flatMap(find) }.first
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

/// Where a conversation's rows are within its visible area (0 to `height`), for `state`
/// (`conversation`): a test checks
/// that rows do not overlap and that the newest one is in view. Measured only while a test drives
/// the app.
@MainActor
enum ChatProbe {
    static var rows: [Int: CGRect] = [:]
    static var visible: CGRect = .zero
    static var content: CGFloat = 0
    static var following = true

    /// Rows in the conversation's order (`order`; a row's id is its first event's sequence number,
    /// negative for a turn's own rows). A row the lazy list keeps aside after it scrolled away may
    /// report where it last was, so only the rows in view are sure to be where they are drawn.
    static var json: JSON {
        let built = rows.sorted { (order[$0.key] ?? 0) < (order[$1.key] ?? 0) }
        return ["following": .bool(following), "content": JSON(Int(content.rounded())),
                "offset": JSON(Int(visible.minY.rounded())), "height": JSON(Int(visible.height.rounded())),
                "rows": JSON.array(built.map { ["id": JSON($0.key), "top": JSON(Int($0.value.minY.rounded())), "bottom": JSON(Int($0.value.maxY.rounded()))] })]
    }

    /// Each row's place in the conversation, top first.
    static var order: [Int: Int] = [:]

    struct Row: ViewModifier {
        let id: Int
        /// Its place in the conversation: ids are not in order (a turn's own rows have negative ones).
        let order: Int
        func body(content: Content) -> some View {
            if Snapshots.active {
                content
                    .onGeometryChange(for: CGRect.self) { $0.frame(in: .scrollView) } action: { ChatProbe.rows[id] = $0; ChatProbe.order[id] = order }
                    .onDisappear { ChatProbe.rows[id] = nil }
            } else {
                content
            }
        }
    }

    struct Scroll: ViewModifier {
        let following: Bool
        func body(content: Content) -> some View {
            if Snapshots.active {
                content
                    .onScrollGeometryChange(for: CGRect.self) { g in CGRect(x: 0, y: g.contentOffset.y, width: g.contentSize.height, height: g.visibleRect.height) } action: { _, r in
                        ChatProbe.visible = CGRect(x: 0, y: r.minY, width: 0, height: r.height)
                        ChatProbe.content = r.width
                    }
                    .onChange(of: following, initial: true) { ChatProbe.following = following }
            } else {
                content
            }
        }
    }
}
