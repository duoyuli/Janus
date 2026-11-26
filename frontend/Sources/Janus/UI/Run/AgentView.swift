import SwiftUI

/// An agent's page in a run: who it is, its subtasks, and its whole conversation.
struct AgentView: View {
    let run: RunModel
    let agentId: String

    var body: some View {
        let st = run.shown
        let w = run.who(agentId)
        let tasks = st.orderedTasks.filter { $0.agent == agentId }
        // A page of its own that scrolls, like the task page: content taller than the window
        // must not push the window's layout around.
        ScrollView {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                Avatar(initials: w.initials, color: w.color, size: 34, role: w.role)
                VStack(alignment: .leading, spacing: 2) {
                    Text(w.name).font(.display(.title2))
                    Text([w.runtime, w.model].filter { !$0.isEmpty }.joined(separator: " · ")).foregroundStyle(.secondary)
                }
                Spacer()
            }
            AdaptiveColumns(side: 300) {
                // A fixed height like the task page: the conversation scrolls inside its card,
                // not the page around it.
                ChatCard(run: run, st: st, title: L.text("对话"), info: agentSession(st, agentId), target: .agent(agentId))
                    .frame(height: 640)
                Card(padding: 12) {
                    HStack { Text(L.text("负责的子任务")).font(.headline); Spacer(); Text(verbatim: L.text("\(tasks.count) 个")).font(.caption).foregroundStyle(.secondary) }
                    if tasks.isEmpty { Text(agentId == "lead" ? L.text("负责人管规划和协调。") : L.text("还没有分配子任务。")).font(.callout).foregroundStyle(.secondary) }
                    ForEach(tasks, id: \.id) { t in
                        let (l, tone) = t.status.label
                        HStack {
                            Button(String("\(t.id) \(t.title)")) { run.show(.task(t.id)) }.buttonStyle(.link).lineLimit(2)
                            Spacer()
                            Pill(l, tone)
                        }
                    }
                }
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        }
    }
}
