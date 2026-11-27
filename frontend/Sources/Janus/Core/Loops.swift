import Foundation

/// Workspace Loops: reading them, making and changing them, waking, pausing and deleting them
/// (`/api/loops`). The Loop's session is a run of kind `loop`, opened like any run.
// @project-doc docs/interfaces/20261031-http_api.md#loops
extension AppStore {
    static func blankLoop(_ workspace: String) -> JSON {
        ["id": "", "workspace": JSON(workspace), "name": "", "prompt": "",
         "schedule": ["cron": JSON(LoopSchedule.daily(hour: 9, minute: 0).cron), "tz": JSON(TimeZone.current.identifier)],
         "scheduleShape": ["kind": "daily", "hour": 9, "minute": 0]]
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

    /// Every Loop, in the order the server lists them (what needs the person first). Read again
    /// whenever the global stream names one: a change of what it is doing can move it in that
    /// order, and a Loop gone from the server goes here too.
    func loadLoops() async {
        if let r = try? await api.get("/api/loops?lang=\(L.serverLanguage)"), r["loops"].array != loops { loops = r["loops"].array }
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
        guard confirm(L.text("删除值班「\(loop["title"].str)」？"), L.text("它的会话和记录一起删掉，找不回来；派出的任务保留。"), ok: L.text("删除")) else { return }
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
        let r = try? await api.get("/api/runs?loop=\(API.q(loop))&limit=100&lang=\(L.serverLanguage)\(cursor)")
        return (r?["runs"].array ?? [], r?["next"].string)
    }
}

/// A Loop's schedule as the form writes it: one of the common shapes it offers, or any other
/// five-field cron expression. The server reads an expression back into its shape and words
/// (`scheduleShape`, `scheduleText`), checks it and says when it comes round.
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

    /// The minutes the time menu offers: every five, and the one a Loop already has.
    static func minuteMarks(keeping minute: Int) -> [Int] {
        Set(stride(from: 0, to: 60, by: 5)).union([minute]).sorted()
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

    /// A day of the week, 0 Sunday.
    static func weekday(_ d: Int) -> String {
        [L.text("周日"), L.text("周一"), L.text("周二"), L.text("周三"), L.text("周四"), L.text("周五"), L.text("周六")][d % 7]
    }
}
