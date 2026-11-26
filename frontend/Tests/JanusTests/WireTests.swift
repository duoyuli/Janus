import Foundation
import Testing
@testable import Janus

/// The strings of one `wire_enum!` in the backend's source (`Name = "value",` lines inside
/// `enum Name {`), wherever under `backend/src` it lives. The app's mirrors are checked against
/// them, so a value added or renamed on one side fails here instead of reading as unknown.
private func backendEnum(_ name: String) throws -> Set<String> {
    let src = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("../../../backend/src").standardizedFileURL
    let files = FileManager.default.enumerator(at: src, includingPropertiesForKeys: nil)?
        .compactMap { $0 as? URL }.filter { $0.pathExtension == "rs" } ?? []
    let head = try Regex("enum \(name) \\{")
    let value = try Regex(#"^\s*\w+ = "([^"]+)",?\s*$"#)
    for file in files {
        let text = try String(contentsOf: file, encoding: .utf8)
        guard let start = text.firstMatch(of: head) else { continue }
        var out: Set<String> = []
        for line in text[start.range.upperBound...].split(separator: "\n", omittingEmptySubsequences: false) {
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("}") { break }
            if let m = line.firstMatch(of: value), let v = m.output[1].substring { out.insert(String(v)) }
        }
        return out
    }
    Issue.record("no enum \(name) under \(src.path)")
    return []
}

@Test func runAndAgentStatesMatchTheBackend() throws {
    #expect(Set(RunKind.allCases.map(\.rawValue)) == (try backendEnum("RunKind")))
    #expect(Set(RunOrigin.allCases.map(\.rawValue)) == (try backendEnum("RunOrigin")))
    #expect(Set(AgentPhase.allCases.map(\.rawValue)) == (try backendEnum("AgentPhase")))
}

/// What a session was given: the system layer's parts (the digital employee's instructions are
/// recorded as `instructions`) and the prompts Janus sends; and how a message went in, which the
/// app also knows as `pending` before the server said.
@Test func sessionChannelsAndDeliveriesMatchTheBackend() throws {
    let channels = try backendEnum("PromptChannel").union(backendEnum("SystemPart")).union(["instructions"])
    #expect(Set(PromptChannel.allCases.map(\.rawValue)) == channels)
    #expect(Set(MessageDelivery.allCases.map(\.rawValue)) == (try backendEnum("Delivery")).union(["pending"]))
}

/// Every action the app sends is one the server takes (`RunAction::name` in `actions.rs`), and a
/// session's actions carry the session the way `SessionTarget` reads it.
@Test func runActionsMatchTheBackend() throws {
    let src = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("../../../backend/src/orchestrator/actions.rs").standardizedFileURL
    let text = try String(contentsOf: src, encoding: .utf8)
    let names = Set(text.matches(of: try Regex(#"RunAction::\w+(?:\s*\{ \.\. \}|\(_\))? => "(\w+)""#)).compactMap { $0.output[1].substring.map(String.init) })
    let lead = SessionTarget.agent("lead")
    let all: [RunAction] = [.answer(key: "", task: "", answer: "", note: ""), .approve(key: "", task: "", agent: "", allow: true, option: nil, label: ""),
                            .input(key: "", task: "", agent: "", action: "accept", content: [:]), .message(lead, text: "hi", images: []),
                            .control(lead, id: "mode", value: "plan", via: "mode"), .interrupt(lead), .takeover(lead), .release(lead), .finish(lead),
                            .pause, .resume, .start, .cancel, .accept, .resumeRun]
    #expect(Set(all.map(\.name)) == names)
    #expect(RunAction.message(.task("T1"), text: "hi", images: []).body == ["type": "message", "task": "T1", "agent": "", "text": "hi"])
    #expect(RunAction.release(lead).body == ["type": "release", "task": "", "agent": "lead"])
}

@Test func janusToolsMatchTheBackend() throws {
    var tools: Set<String> = []
    for name in ["LeadTool", "MemberTool", "LoopTool", "MemoryTool", "ChatTool"] { tools.formUnion(try backendEnum(name)) }
    #expect(Set(JanusTool.allCases.map(\.rawValue)) == tools)
    #expect(JanusTool(call: "janus.ask_user") == .askUser)
    #expect(JanusTool(call: "ask_user") == nil && JanusTool(call: "janus.nope") == nil)
}
