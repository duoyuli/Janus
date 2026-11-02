import Foundation

/// Display tones shared by pills, dots and event icons.
enum Tone: String { case gray, blue, green, amber, red, violet }

/// Labels and small formatting helpers.
enum L {
    static let taskStatus: [String: (String, Tone)] = [
        "pending": ("等待依赖", .gray), "ready": ("就绪", .gray), "assigned": ("已分配", .blue), "running": ("执行中", .blue),
        "waiting_input": ("等你回答", .amber), "waiting_approval": ("等你审批", .amber),
        "blocked": ("依赖失败", .amber), "skipped_by_decision": ("主控取消", .gray),
        "verifying": ("核对中", .violet), "verification_failed": ("没通过验收", .red), "succeeded": ("已完成", .green), "failed": ("失败", .red), "cancelled": ("已取消", .gray),
    ]
    static func task(_ s: String) -> (String, Tone) { taskStatus[s] ?? (s, .gray) }

    static let agentPhase: [String: (String, Tone)] = [
        "idle": ("空闲", .gray), "thinking": ("规划中", .blue), "coordinating": ("协调中", .blue), "reading_files": ("读取文件", .blue),
        "editing": ("修改文件", .blue), "running_tool": ("调用工具", .blue), "running_test": ("运行测试", .blue),
        "waiting_for_input": ("等待输入", .amber), "waiting_for_agent": ("等待协作者", .amber), "waiting_approval": ("等待审批", .amber), "error": ("异常", .red),
    ]
    static func phase(_ s: String) -> (String, Tone) { agentPhase[s] ?? agentPhase["idle"]! }

    static let attemptStatus: [String: (String, Tone)] = ["running": ("进行中", .blue), "succeeded": ("成功", .green), "failed": ("失败", .red)]

    static let runListStatus: [String: (String, Tone)] = [
        "planning": ("主控启动中", .blue), "waiting_input": ("等你追加预算", .amber), "running": ("执行中", .blue),
        "delivered": ("已交付", .green), "accepted": ("已验收", .green), "failed": ("失败", .red),
        "cancelled": ("已停止", .gray), "interrupted": ("已中断", .gray),
    ]

    static let artKind = ["report": "回复", "git_diff": "代码改动", "test_report": "测试结果"]

    static let icon: [String: String] = [
        "run.created": "✦", "run.cancelled": "■", "run.paused": "Ⅱ", "run.resumed": "▶",
        "task.ready": "▸", "task.assigned": "↗", "attempt.started": "▶", "attempt.failed": "✕", "task.retry_scheduled": "↻", "task.reopened": "↺",
        "result.submitted": "⇪", "task.succeeded": "✓", "artifact.created": "▤", "context.handoff": "⇄",
        "question.asked": "?", "question.escalated": "⇡", "question.answered": "✎", "answer.delivered": "↩",
        "approval.requested": "⚑", "input.requested": "✎", "input.answered": "↩", "approval.granted": "✓", "approval.denied": "✕",
        "run.delivered": "★", "run.budget_exhausted": "⌛", "risk.accepted": "✓", "task.failed": "✕",
        "task.blocked": "⊘", "task.skipped_by_decision": "→", "verification.started": "◌", "verification.failed": "✕",
        "verification.passed": "✓", "run.failed": "✕", "run.interrupted": "■", "agent.status": "·", "tool.call": "⌘", "tool.started": "⌘", "tool.completed": "✓",
        "session.prompt": "→", "result.amended": "✎", "run.recovered": "↻", "message.routed": "↪", "decision.withdrawn": "↩",
        "run.budget_extended": "⌛", "agent.message": "❝", "user.message": "✎",
        "message.delivered": "↪", "control.taken": "✋", "control.released": "↻", "session.interrupted": "‖",
    ]

    private static let toneTable: [Tone: [String]] = [
        .green: ["task.succeeded", "approval.granted", "input.answered", "run.delivered", "risk.accepted"],
        .amber: ["question.asked", "question.escalated", "approval.requested", "input.requested", "run.paused", "control.taken", "session.interrupted", "user.message"],
        .red: ["attempt.failed", "approval.denied", "run.cancelled", "task.failed", "run.failed", "run.interrupted"],
        .gray: ["agent.status", "tool.call", "tool.completed", "task.ready"],
        .blue: ["tool.started", "session.prompt"],
    ]
    static func tone(of type: String) -> Tone? { toneTable.first { $0.value.contains(type) }?.key }

    /// Event categories for the feed and the audit filter.
    static func category(_ type: String) -> String {
        func m(_ p: String) -> Bool { type.range(of: p, options: .regularExpression) != nil }
        if m(#"^(question\.answered|input\.answered|approval\.(granted|denied)|risk\.|run\.(paused|resumed|cancelled))"#) { return "decisions" }
        if m(#"^(question\.|approval\.requested|input\.requested|answer\.|user\.message|message\.delivered|control\.|session\.)"#) { return "messages" }
        if type == "agent.message" { return "progress" }
        if type.hasPrefix("artifact.") || type == "context.handoff" { return "artifacts" }
        if m(#"^(agent\.status|tool\.|session\.prompt)"#) { return "progress" }
        return "tasks"
    }
    static let categoryLabel: [(String, String)] = [("all", "全部"), ("tasks", "任务"), ("messages", "消息"), ("artifacts", "产物"), ("decisions", "决策"), ("progress", "进度与工具")]

    // MARK: Time

    static func clock(_ ms: Double) -> String {
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss"
        return f.string(from: Date(timeIntervalSince1970: ms / 1000))
    }
    static func dateTime(_ ms: Double) -> String {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .short
        return f.string(from: Date(timeIntervalSince1970: ms / 1000))
    }
    static func dur(_ ms: Double) -> String {
        let s = max(0, Int((ms / 1000).rounded()))
        return s < 60 ? "\(s) 秒" : "\(s / 60) 分 \(String(format: "%02d", s % 60)) 秒"
    }
    static func ago(_ ts: Double, _ now: Double) -> String {
        let s = max(0, Int(((now - ts) / 1000).rounded()))
        return s < 60 ? "\(s)s 前" : "\(s / 60) 分钟前"
    }
    static func nowMs() -> Double { Date().timeIntervalSince1970 * 1000 }

    // MARK: Text

    /// Home-relative path for display.
    static func tilde(_ p: String) -> String {
        p.replacingOccurrences(of: #"^/(?:Users|home)/[^/]+/"#, with: "~/", options: .regularExpression)
    }
    /// Markdown marks dropped, for previews cut to a length.
    static func plain(_ t: String) -> String {
        t.replacingOccurrences(of: #"\[([^\]\n]+)\]\([^)\s]+\)"#, with: "$1", options: .regularExpression)
            .replacingOccurrences(of: #"\*\*|`"#, with: "", options: .regularExpression)
            .replacingOccurrences(of: #"(?m)^#+\s*"#, with: "", options: .regularExpression)
    }
    /// A goal can be a long pasted brief; lists and headers show its first line.
    static func goalTitle(_ g: String) -> String {
        let first = g.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.first { !$0.isEmpty } ?? ""
        return plain(first).trimmingCharacters(in: .whitespaces)
    }
    static func short(_ t: String, _ n: Int = 26) -> String { t.count > n ? String(t.prefix(n)) + "…" : t }
    static func kb(_ n: Int?) -> String {
        guard let n else { return "" }
        return n < 1024 ? "\(n) B" : String(format: n < 10240 ? "%.1f KB" : "%.0f KB", Double(n) / 1024)
    }
}
