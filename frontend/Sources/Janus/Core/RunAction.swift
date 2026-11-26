import Foundation

/// A person's action on a run (`POST /api/runs/{id}/actions`), one case per action the server
/// takes (`backend/src/orchestrator/actions.rs`, `RunAction`). The request body is built here
/// only, so every button that answers, approves or steers a session sends the same fields.
// @project-doc docs/interfaces/20261031-http_api.md#run_actions
enum RunAction: Equatable {
    /// Answer a question a task's agent (`task`), or the lead (no task), is waiting on.
    case answer(key: String, task: String, answer: String, note: String)
    /// Answer an agent's form: `accept`, `decline` or `cancel`, with what was filled in.
    case input(key: String, task: String, agent: String, action: String, content: [String: JSON])
    case message(SessionTarget, text: String, images: [JSON])
    /// Switch one of an open session's controls.
    case control(SessionTarget, id: String, value: String, via: String)
    case interrupt(SessionTarget), takeover(SessionTarget), release(SessionTarget), finish(SessionTarget)
    case pause, resume, start, cancel, accept, resumeRun
    /// A request body the server prepared (a quick answer, `QuickAction.body`), sent as it is.
    case served(JSON)

    /// Its `type`, as the server spells it.
    var name: String {
        switch self {
        case .answer: return "answer"
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
        case .served(let b): return b["type"].str
        }
    }

    /// Whether it brings a run that is not live back to work: continuing it, or talking to it.
    var reopens: Bool {
        switch self {
        case .resumeRun, .message: return true
        default: return false
        }
    }

    var body: JSON {
        var b: JSON
        switch self {
        case let .answer(key, task, answer, note):
            b = ["key": JSON(key), "task": JSON(task), "answer": JSON(answer), "note": JSON(note)]
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
        case .served(let body):
            return body
        }
        b["type"] = JSON(name)
        return b
    }
}
