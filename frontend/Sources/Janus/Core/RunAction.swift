import Foundation

/// A person's action on a run (`POST /api/runs/{id}/actions`), one case per action the server
/// takes (`backend/src/orchestrator/actions.rs`, `RunAction`). The request body is built here
/// only, so every button that answers, approves or steers a session sends the same fields.
// @project-doc docs/interfaces/20261031-http_api.md#run_actions
enum RunAction: Equatable {
    /// Answer a question a task's agent (`task`), or the lead (no task), is waiting on.
    case answer(key: String, task: String, answer: String, note: String)
    /// Decide an agent's approval request; `option` is the agent's own `optionId`, `label` its words.
    case approve(key: String, task: String, agent: String, allow: Bool, option: String?, label: String)
    /// Answer an agent's form: `accept`, `decline` or `cancel`, with what was filled in.
    case input(key: String, task: String, agent: String, action: String, content: [String: JSON])
    case message(SessionTarget, text: String, images: [JSON])
    /// Switch one of an open session's controls.
    case control(SessionTarget, id: String, value: String, via: String)
    case interrupt(SessionTarget), takeover(SessionTarget), release(SessionTarget), finish(SessionTarget)
    case pause, resume, start, cancel, accept, resumeRun

    /// Its `type`, as the server spells it.
    var name: String {
        switch self {
        case .answer: return "answer"
        case .approve: return "approve"
        case .input: return "input"
        case .message: return "message"
        case .control: return "control"
        case .interrupt: return "interrupt"
        case .takeover: return "takeover"
        case .release: return "release"
        case .finish: return "finish"
        case .pause: return "pause"
        case .resume: return "resume"
        case .start: return "start"
        case .cancel: return "cancel"
        case .accept: return "accept"
        case .resumeRun: return "resumeRun"
        }
    }

    /// Whether it brings a run that is not live back to work: continuing it, or talking to it.
    var reopens: Bool {
        switch self {
        case .resumeRun, .message: return true
        default: return false
        }
    }

    /// The answer to an approval request with one of the agent's options.
    static func approve(_ item: InboxItem, _ o: L.PermissionOption) -> RunAction {
        .approve(key: item.key, task: item.task ?? "", agent: item.agent ?? "", allow: o.kind.hasPrefix("allow"), option: o.id.isEmpty ? nil : o.id, label: o.name)
    }

    var body: JSON {
        var b: JSON
        switch self {
        case let .answer(key, task, answer, note):
            b = ["key": JSON(key), "task": JSON(task), "answer": JSON(answer), "note": JSON(note)]
        case let .approve(key, task, agent, allow, option, label):
            b = ["key": JSON(key), "task": JSON(task), "agent": JSON(agent), "allow": JSON(allow), "option": option.map(JSON.init) ?? .null, "label": JSON(label)]
        case let .input(key, task, agent, action, content):
            b = ["key": JSON(key), "task": JSON(task), "agent": JSON(agent), "action": JSON(action), "content": .object(content)]
        case let .message(to, text, images):
            b = to.body
            b["text"] = JSON(text)
            if !images.isEmpty { b["images"] = .array(images) }
        case let .control(to, id, value, via):
            b = to.body
            b["id"] = JSON(id); b["value"] = JSON(value); b["via"] = JSON(via)
        case .interrupt(let to), .takeover(let to), .release(let to), .finish(let to):
            b = to.body
        case .pause, .resume, .start, .cancel, .accept, .resumeRun:
            b = [:]
        }
        b["type"] = JSON(name)
        return b
    }
}
