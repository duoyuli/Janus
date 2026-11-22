import Foundation

/// Workspace Loops: reading them, making and changing them, waking, pausing and deleting them
/// (`/api/loops`). The Loop's session is a run of kind `loop`, opened like any run.
// @project-doc docs/interfaces/20261031-http_api.md#loops
extension AppStore {
    static func blankLoop(_ workspace: String) -> JSON {
        ["id": "", "workspace": JSON(workspace), "name": "", "prompt": "",
         "schedule": ["cron": JSON(LoopSchedule.daily(hour: 9, minute: 0).cron), "tz": JSON(TimeZone.current.identifier)]]
    }

    /// When a schedule comes round next (`times`, ms), as the server reads it in this Mac's time
    /// zone; `error` says why it is not a schedule.
    func preview(cron: String) async -> (times: [Double], error: String?) {
        do {
            let r = try await api.get("/api/loops/schedule?cron=\(API.q(cron))&tz=\(API.q(TimeZone.current.identifier))")
            return (r["next"].array.compactMap(\.double), nil)
        } catch let e as APIError {
            return ([], e.message)
        } catch {
            return ([], error.localizedDescription)
        }
    }

    func loadLoops() async {
        if let r = try? await api.get("/api/loops"), r["loops"].array != loops { loops = r["loops"].array }
    }

    /// Read one Loop again (the global stream named it); gone from the server, it goes here too.
    func loadLoop(_ id: String) async {
        guard !id.isEmpty else { return }
        do {
            let l = try await api.get("/api/loops/\(API.q(id))")
            if let i = loops.firstIndex(where: { $0["id"].str == id }) {
                if loops[i] != l { loops[i] = l }
            } else {
                loops.append(l)
            }
        } catch let e as APIError where e.status == 404 {
            loops.removeAll { $0["id"].str == id }
        } catch {}
    }

    func loops(of workspace: String) -> [JSON] { loops.filter { $0["workspace"].str == workspace } }

    /// The Loop whose session is run `id`.
    func loop(ofRun id: String) -> JSON? { loops.first { $0["run"].str == id } }

    /// Make a Loop (`original` without an id) or change one; `false` when the server refused.
    func saveLoop(_ original: JSON, _ body: JSON) async -> Bool {
        do {
            if original["id"].str.isEmpty {
                let made = try await api.post("/api/workspaces/\(API.q(original["workspace"].str))/loops", body)
                await loadLoops()
                await loadRuns()
                open(.run(made["run"].str))
            } else {
                _ = try await api.put("/api/loops/\(API.q(original["id"].str))", body)
                await loadLoops()
            }
            return true
        } catch {
            show(L.text("保存值班失败：\(error.localizedDescription)"))
            return false
        }
    }

    /// Wake, pause or resume a Loop.
    func loopAction(_ loop: JSON, _ action: String) async {
        do {
            _ = try await api.post("/api/loops/\(API.q(loop["id"].str))/\(action)", [:])
            await loadLoops()
        } catch { show(error.localizedDescription) }
    }

    /// Delete a Loop with its session; the runs it dispatched stay.
    func deleteLoop(_ loop: JSON) async {
        guard confirm(L.text("删除值班「\(loop["title"].str)」？"), L.text("它的会话和记录一起删除，不可恢复；它派出的任务保留。"), ok: L.text("删除")) else { return }
        do {
            try await api.delete("/api/loops/\(API.q(loop["id"].str))")
            if run?.id == loop["run"].str { open(.home(loop["workspace"].str)) }
            await loadLoops()
            await loadRuns()
        } catch { show(error.localizedDescription) }
    }

    /// A page of the runs a Loop dispatched, newest first, and where the next page starts.
    func dispatched(by loop: String, before: String? = nil) async -> (runs: [JSON], next: String?) {
        guard !loop.isEmpty else { return ([], nil) }
        let cursor = before.map { "&cursor=\(API.q($0))" } ?? ""
        let r = try? await api.get("/api/runs?loop=\(API.q(loop))&limit=100\(cursor)")
        return (r?["runs"].array ?? [], r?["next"].string)
    }
}

/// A Loop's schedule as the app reads and writes it: one of the common shapes the form offers, or
/// any other five-field cron expression. The server checks it and says when it comes round.
enum LoopSchedule: Equatable {
    /// Every `m` minutes, on the clock.
    case minutes(Int)
    /// Every `h` hours, on the hour.
    case hours(Int)
    case daily(hour: Int, minute: Int)
    case weekdays(hour: Int, minute: Int)
    /// On one day of the week, 0 Sunday.
    case weekly(day: Int, hour: Int, minute: Int)
    /// On one day of the month.
    case monthly(day: Int, hour: Int, minute: Int)
    case cron(String)

    static let minuteChoices = [5, 10, 15, 20, 30]
    static let hourChoices = [1, 2, 3, 4, 6, 8, 12]

    /// The shape a cron expression has, `.cron` when it is none of the common ones.
    init(cron: String) {
        let f = cron.split(separator: " ").map(String.init)
        self = .cron(f.joined(separator: " "))
        guard f.count == 5, f[3] == "*" else { return }
        let (mi, h, d, w) = (f[0], f[1], f[2], f[4])
        let every = { (s: String) in s.hasPrefix("*/") ? Int(s.dropFirst(2)) : nil }
        if let m = every(mi), h == "*", d == "*", w == "*", Self.minuteChoices.contains(m) { self = .minutes(m); return }
        guard let m = Int(mi), (0..<60).contains(m) else { return }
        if m == 0, h == "*", d == "*", w == "*" { self = .hours(1); return }
        if m == 0, let x = every(h), d == "*", w == "*", Self.hourChoices.contains(x) { self = .hours(x); return }
        guard let hh = Int(h), (0..<24).contains(hh) else { return }
        switch (d, w) {
        case ("*", "*"): self = .daily(hour: hh, minute: m)
        case ("*", "1-5"): self = .weekdays(hour: hh, minute: m)
        case ("*", _): if let x = Int(w), (0...6).contains(x) { self = .weekly(day: x, hour: hh, minute: m) }
        case (_, "*"): if let x = Int(d), (1...31).contains(x) { self = .monthly(day: x, hour: hh, minute: m) }
        default: break
        }
    }

    var cron: String {
        switch self {
        case .minutes(let m): "*/\(m) * * * *"
        case .hours(1): "0 * * * *"
        case .hours(let h): "0 */\(h) * * *"
        case .daily(let h, let m): "\(m) \(h) * * *"
        case .weekdays(let h, let m): "\(m) \(h) * * 1-5"
        case .weekly(let d, let h, let m): "\(m) \(h) * * \(d)"
        case .monthly(let d, let h, let m): "\(m) \(h) \(d) * *"
        case .cron(let c): c
        }
    }

    /// The schedule in words: every 30 minutes, weekdays at 09:00, …
    var words: String {
        switch self {
        case .minutes(let m): L.text("每 \(m) 分钟")
        case .hours(1): L.text("每小时")
        case .hours(let h): L.text("每 \(h) 小时")
        case .daily(let h, let m): L.text("每天 \(Self.time(h, m))")
        case .weekdays(let h, let m): L.text("工作日 \(Self.time(h, m))")
        case .weekly(let d, let h, let m): L.text("每\(Self.weekday(d)) \(Self.time(h, m))")
        case .monthly(let d, let h, let m): L.text("每月 \(d) 日 \(Self.time(h, m))")
        case .cron(let c): L.text("cron：\(c)")
        }
    }

    static func time(_ h: Int, _ m: Int) -> String { String(format: "%02d:%02d", h, m) }

    /// A day of the week, 0 Sunday.
    static func weekday(_ d: Int) -> String {
        [L.text("周日"), L.text("周一"), L.text("周二"), L.text("周三"), L.text("周四"), L.text("周五"), L.text("周六")][d % 7]
    }
}
