import Foundation

/// What sessions are writing and running right now, before it is on record: the run's event
/// stream carries `live` frames beside its events. A frame only previews; the events stay what
/// the conversation is built from, so a frame shows only while no newer record of the same
/// output has arrived, and the end of a turn drops whatever it previewed.
// @project-doc docs/architecture/20261031-frontend.md#live_preview
enum ChatLive {
    /// A running tool call as its newest frame describes it.
    struct Tool: Equatable {
        var title: String
        var tool: String
        var input: String
        var output: String
        var status: String
    }

    /// One session's preview: its text and thought not yet recorded, its running calls.
    struct Session: Equatable {
        var text = ""
        var thought = ""
        /// The run's newest event when the text was previewed.
        var seq = 0
        var tools: [String: Tool] = [:]
        var isEmpty: Bool { text.isEmpty && thought.isEmpty && tools.isEmpty }
    }

    /// Every session's preview of one run, and what the records already cover.
    struct State: Equatable {
        var sessions: [Key: Session] = [:]
        /// The newest recorded text or thought of each session, by seq.
        var said: [Key: Int] = [:]
        /// Tool calls already on record, by session and call id.
        var done: Set<Call> = []

        init() {}

        /// What `events` already cover, after the run is read again.
        init(_ events: [Event]) {
            for e in events { recorded(e) }
            sessions = [:]
        }

        /// A new event: the text, thought or call it records is no longer a preview.
        mutating func recorded(_ e: Event) {
            switch e.type {
            case .agentMessage, .agentThought:
                let k = Key(actor: e.actor, task: e.task)
                said[k] = e.seq
                if let s = sessions[k], e.seq > s.seq {
                    sessions[k]?.text = ""
                    sessions[k]?.thought = ""
                }
            case .toolCall:
                guard e.data["callId"].truthy else { return }
                let k = Key(actor: e.actor, task: e.task)
                done.insert(Call(session: k, id: e.data["callId"].str))
                sessions[k]?.tools[e.data["callId"].str] = nil
            default: break
            }
        }

        /// A frame from the stream.
        mutating func apply(_ f: JSON) {
            let k = Key(actor: f["actor"].str, task: f["task"].string)
            let seq = f["seq"].int ?? 0
            switch f["kind"].str {
            case "say":
                // Made before the newest record of this session: that record already holds it.
                if let s = said[k], seq < s { return }
                var s = sessions[k] ?? Session()
                s.text = f["text"].str
                s.thought = f["thought"].str
                s.seq = seq
                sessions[k] = s
            case "tool":
                let id = f["callId"].str
                guard !id.isEmpty, !done.contains(Call(session: k, id: id)) else { return }
                sessions[k, default: Session()].tools[id] = Tool(title: f["title"].str, tool: f["tool"].str, input: f["input"].str, output: f["output"].str, status: f["status"].str)
            case "end":
                sessions[k] = nil
            default: break
            }
        }

        /// The newest frame of a call still running.
        func tool(actor: String, task: String?, id: String) -> Tool? {
            sessions[Key(actor: actor, task: task)]?.tools[id]
        }

        /// The previews of a conversation: a task's session, or one agent's.
        func of(task: String?, agent: String?) -> Session? {
            if let task { return sessions.first { $0.key.task == task }?.value }
            if let agent { return sessions[Key(actor: agent, task: nil)] ?? sessions.first { $0.key.actor == agent }?.value }
            return nil
        }
    }

    /// A session: the agent speaking and the task it works on (none for the lead's own).
    struct Key: Hashable {
        let actor: String
        let task: String?
    }

    struct Call: Hashable {
        let session: Key
        let id: String
    }
}
