import Foundation

/// Display tones shared by pills, dots and event icons.
enum Tone: String { case gray, blue, green, amber, red, violet }

/// Labels and small formatting helpers, the same wording as the web page.
enum L {
    static let taskStatus: [String: (String, Tone)] = [
        "created": ("待确认", .gray), "pending": ("等待依赖", .gray), "ready": ("就绪", .gray), "assigned": ("已分配", .blue), "running": ("执行中", .blue),
        "waiting_input": ("等你回答", .amber), "waiting_approval": ("等你审批", .amber), "waiting_review": ("待评审", .violet),
        "blocked": ("依赖失败", .amber), "cancelling": ("清理中", .amber), "cancelled_by_dependency": ("因依赖取消", .gray), "skipped_by_decision": ("按决定跳过", .gray),
        "verifying": ("核对中", .violet), "verification_failed": ("没通过验收", .red), "succeeded": ("已完成", .green), "failed": ("失败", .red), "cancelled": ("已取消", .gray),
    ]
    static func task(_ s: String) -> (String, Tone) { taskStatus[s] ?? (s, .gray) }

    static let agentPhase: [String: (String, Tone)] = [
        "idle": ("空闲", .gray), "thinking": ("规划中", .blue), "coordinating": ("协调中", .blue), "reading_files": ("读取文件", .blue),
        "editing": ("修改文件", .blue), "running_tool": ("调用工具", .blue), "running_test": ("运行测试", .blue),
        "waiting_for_input": ("等待输入", .amber), "waiting_for_agent": ("等待协作者", .amber), "waiting_approval": ("等待审批", .amber), "error": ("异常", .red),
    ]
    static func phase(_ s: String) -> (String, Tone) { agentPhase[s] ?? agentPhase["idle"]! }

    static let attemptStatus: [String: (String, Tone)] = ["running": ("进行中", .blue), "succeeded": ("成功", .green), "failed": ("失败", .red), "rejected": ("被评审驳回", .red), "verdict": ("给出驳回意见", .amber)]

    static let runListStatus: [String: (String, Tone)] = [
        "planning": ("规划中", .blue), "awaiting_plan": ("等你确认计划", .amber), "waiting_input": ("等你决策", .amber), "running": ("执行中", .blue),
        "delivered": ("已交付", .green), "accepted": ("已验收", .green), "accepted_with_risk": ("已接受风险", .amber), "failed": ("失败", .red),
        "cancelled": ("已取消", .gray), "interrupted": ("已中断", .gray),
    ]

    static let diagCats: [(String, String)] = [("input", "用户输入不足"), ("plan", "任务拆解不合理"), ("capability", "Agent 能力不匹配"), ("tool", "工具或环境不足"), ("data", "数据源缺失"), ("permission", "权限限制"), ("execution", "Agent 执行偏差"), ("review", "评审遗漏")]
    static let wsLabel = ["shared_readonly": "共享只读目录", "readonly_commit": "只读，指定 commit", "isolated_worktree": "独立 worktree", "orchestrator": "主控工作区", "workspace": "直接在工作目录中"]
    static let artKind = ["report": "回复", "git_diff": "代码改动", "test_report": "测试结果"]

    static let icon: [String: String] = [
        "run.created": "✦", "plan.proposed": "◇", "plan.approved": "✓", "run.cancelled": "■", "run.paused": "Ⅱ", "run.resumed": "▶",
        "task.ready": "▸", "task.assigned": "↗", "attempt.started": "▶", "attempt.failed": "✕", "task.retry_scheduled": "↻", "task.reopened": "↺",
        "result.submitted": "⇪", "task.succeeded": "✓", "dependency.resolved": "⇢", "artifact.created": "▤", "artifact.handoff": "⇄", "context.handoff": "⇄",
        "question.asked": "?", "question.escalated": "⇡", "question.answered": "✎", "answer.delivered": "↩",
        "approval.requested": "⚑", "input.requested": "✎", "input.answered": "↩", "approval.granted": "✓", "approval.denied": "✕", "review.rejected": "✕", "review.approved": "✓",
        "run.delivered": "★", "run.waiting_input": "?", "run.budget_exhausted": "⌛", "run.accepted_with_risk": "⚠", "risk.accepted": "✓", "fix.requested": "↻", "task.failed": "✕",
        "task.blocked": "⊘", "task.cancelled_by_dependency": "■", "task.skipped_by_decision": "→", "task.cancel_requested": "‖", "verification.started": "◌", "verification.failed": "✕",
        "verification.passed": "✓", "run.failed": "✕", "run.interrupted": "■", "policy.violation": "⚠", "agent.status": "·", "tool.call": "⌘", "tool.started": "⌘", "tool.completed": "✓",
        "session.prompt": "→", "decision.summary": "◆", "result.amended": "✎", "run.recovered": "↻", "message.routed": "↪", "decision.withdrawn": "↩", "plan.validation_exhausted": "✕",
        "plan.replan_requested": "✎", "verification.command_rejected": "⚠", "run.budget_extended": "⌛", "plan.revision_requested": "✎", "agent.message": "❝", "user.message": "✎",
        "message.delivered": "↪", "control.taken": "✋", "control.released": "↻", "session.interrupted": "‖",
    ]

    private static let toneTable: [Tone: [String]] = [
        .green: ["plan.approved", "task.succeeded", "review.approved", "approval.granted", "input.answered", "run.delivered", "risk.accepted"],
        .amber: ["question.asked", "question.escalated", "approval.requested", "input.requested", "plan.proposed", "run.paused", "control.taken", "session.interrupted", "user.message"],
        .red: ["attempt.failed", "review.rejected", "approval.denied", "run.cancelled", "task.failed", "run.failed", "run.interrupted", "policy.violation"],
        .gray: ["agent.status", "tool.call", "tool.completed", "dependency.resolved", "task.ready"],
        .blue: ["tool.started", "session.prompt"],
    ]
    static func tone(of type: String) -> Tone? { toneTable.first { $0.value.contains(type) }?.key }

    /// Event categories for the feed and the audit filter.
    static func category(_ type: String) -> String {
        func m(_ p: String) -> Bool { type.range(of: p, options: .regularExpression) != nil }
        if m(#"^(plan\.approved|question\.answered|input\.answered|approval\.(granted|denied)|review\.|risk\.|fix\.|run\.(paused|resumed|cancelled))"#) { return "decisions" }
        if m(#"^(question\.|approval\.requested|input\.requested|answer\.|user\.message|message\.delivered|control\.|session\.)"#) { return "messages" }
        if type == "agent.message" { return "progress" }
        if type.hasPrefix("artifact.") || type == "context.handoff" { return "artifacts" }
        if m(#"^(agent\.status|tool\.|session\.prompt|decision\.summary)"#) { return "progress" }
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
