import AppKit
import SwiftUI

/// The Skill library: GitHub repositories added by link, whose skills are found automatically, and
/// the skills written or imported into Janus itself. Roles pick from all of them.
// @project-doc docs/domains/agents_and_roles.md#skill_sources
struct SkillsView: View {
    let store: AppStore
    @Local private var link = ""
    @Local private var adding = false
    @Local private var checking = false
    @Local private var open: Set<String> = []
    @Local private var query = ""

    var body: some View {
        let sources = store.skillSources ?? []
        let own = (store.skills ?? []).filter { !$0["sourceId"].truthy }
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("粘贴 GitHub 仓库链接，Janus 会拉取仓库并发现其中所有的 Skill（含 SKILL.md 的文件夹）。之后在角色里选择，角色启动的 Agent 就能使用它们。").font(.callout).foregroundStyle(.secondary)
                HStack {
                    TextField("https://github.com/owner/repo 或 …/tree/main/skills", text: $link)
                        .textFieldStyle(.roundedBorder)
                        .onSubmit(add)
                        .disabled(adding)
                    Button(adding ? "正在拉取…" : "添加") { add() }
                        .buttonStyle(.borderedProminent)
                        .disabled(adding || link.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                if adding { Note(text: "正在克隆仓库并查找 Skill，较大的仓库可能需要一分钟。") }
                if !sources.isEmpty { updateBar }
                if store.skillSources == nil { ProgressView() }
                if !sources.isEmpty || !own.isEmpty {
                    TextField("搜索 Skill", text: $query).textFieldStyle(.roundedBorder)
                }
                ForEach(sources, id: \.self) { s in source(s) }
                ownSkills(own)
            }
            .padding(20)
        }
        .task {
            async let a: Void = store.loadSkillSources()
            async let b: Void = store.loadSkills()
            _ = await (a, b)
        }
    }

    // MARK: Sources

    private func source(_ s: JSON) -> some View {
        let id = s["id"].str
        let busy = store.updatingSources.contains(id)
        let skills = filtered(s["skills"].array)
        let expanded = open.contains(id) || !query.isEmpty
        let where_ = [s["ref"].truthy ? s["ref"].str : "", s["path"].str].filter { !$0.isEmpty }.joined(separator: " · ")
        return Card {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "shippingbox").foregroundStyle(Color.accent).padding(.top, 2)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Button(s["repo"].str) { if let u = URL(string: s["url"].str) { NSWorkspace.shared.open(u) } }
                            .buttonStyle(.link).fontWeight(.semibold).help(s["url"].str)
                        Pill("\(s["skills"].array.count) Skill")
                        if s["updateAvailable"].truthy { Pill("有更新", .blue).help("GitHub 上的最新提交是 \(String(s["latest"].str.prefix(7)))") }
                        else if s["checkError"].truthy { Pill("检查失败", .amber).help(s["checkError"].str) }
                    }
                    Text(([where_, s["commit"].truthy ? String(s["commit"].str.prefix(7)) : "", s["updatedAt"].truthy ? "\(L.dateTime(s["updatedAt"].double ?? 0)) 拉取" : "", s["checkedAt"].truthy ? "\(L.dateTime(s["checkedAt"].double ?? 0)) 检查" : ""].filter { !$0.isEmpty }).joined(separator: " · "))
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if busy { ProgressView().controlSize(.small) }
                if s["updateAvailable"].truthy {
                    Button("更新") { update(id) }.buttonStyle(.borderedProminent).disabled(busy).help("拉取最新内容，Skill 跟随仓库更新")
                } else {
                    Button("重新拉取") { update(id) }.disabled(busy).help("重新拉取仓库")
                }
                Button { remove(s) } label: { Label("删除来源", systemImage: "trash") }
                    .labelStyle(.iconOnly).buttonStyle(.borderless).disabled(busy).help("删除来源")
            }
            Button {
                if open.contains(id) { open.remove(id) } else { open.insert(id) }
            } label: {
                Label(expanded ? "收起" : "查看 Skill", systemImage: expanded ? "chevron.down" : "chevron.right").font(.callout)
            }
            .buttonStyle(.plain).foregroundStyle(.secondary)
            if expanded {
                if skills.isEmpty { Text("没有匹配的 Skill").font(.callout).foregroundStyle(.secondary) }
                ForEach(skills, id: \.self) { k in skillRow(k, detail: k["relPath"].str) }
            }
        }
    }

    /// Reminder and one-click update for every source with news, or a manual check.
    private var updateBar: some View {
        let updates = store.skillSourceUpdates
        let busy = !store.updatingSources.isEmpty
        return HStack(spacing: 10) {
            Image(systemName: updates.isEmpty ? "checkmark.circle" : "arrow.down.circle.fill")
                .foregroundStyle(updates.isEmpty ? Color.secondary : Tone.blue.color)
            Text(updates.isEmpty ? "Janus 每 6 小时检查一次 GitHub 上的更新。" : "\(updates.map { $0["repo"].str }.joined(separator: "、")) 有新的提交。")
                .font(.callout).foregroundStyle(updates.isEmpty ? .secondary : .primary)
            Spacer()
            if checking || busy { ProgressView().controlSize(.small) }
            Button(checking ? "正在检查…" : "检查更新") {
                checking = true
                Task { await store.checkSkillSources(); checking = false }
            }
            .disabled(checking || busy)
            if !updates.isEmpty {
                Button(updates.count == 1 ? "更新" : "全部更新（\(updates.count)）") { Task { await store.updateSkillSources() } }
                    .buttonStyle(.borderedProminent).disabled(busy)
            }
        }
        .padding(10)
        .background((updates.isEmpty ? Color.secondary : Tone.blue.color).opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
    }

    // MARK: Own skills

    @ViewBuilder private func ownSkills(_ own: [JSON]) -> some View {
        let list = filtered(own)
        Card {
            HStack {
                Image(systemName: "tray.full").foregroundStyle(Color.accent)
                Text("Janus 自建").fontWeight(.semibold)
                Pill("\(own.count) Skill")
                Spacer()
                Button("从文件夹导入…") { importFolder() }.buttonStyle(.link)
            }
            Text("在角色编辑器中新建的，或从本机文件夹复制进来的 Skill。").font(.caption).foregroundStyle(.secondary)
            if list.isEmpty { Text(own.isEmpty ? "还没有" : "没有匹配的 Skill").font(.callout).foregroundStyle(.secondary) }
            ForEach(list, id: \.self) { k in
                HStack {
                    skillRow(k, detail: "")
                    Button { deleteOwn(k) } label: { Label("删除 \(k["name"].str)", systemImage: "trash") }
                        .labelStyle(.iconOnly).buttonStyle(.borderless).help("删除")
                }
            }
        }
    }

    private func skillRow(_ k: JSON, detail: String) -> some View {
        let users = store.roles.filter { $0["skills"].strings.contains(k["id"].str) }.map { $0["name"].str }
        return HStack(alignment: .top, spacing: 8) {
            Image(systemName: "sparkles").foregroundStyle(.secondary).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 6) {
                    Text(k["name"].str).fontWeight(.medium)
                    if !users.isEmpty { Pill(users.joined(separator: "、"), .violet).help("挂载这个 Skill 的角色") }
                }
                if k["description"].truthy { Text(k["description"].str).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                if !detail.isEmpty { Text(detail).font(.system(.caption2, design: .monospaced)).foregroundStyle(.tertiary) }
            }
            Spacer()
        }
        .padding(.vertical, 3)
    }

    private func filtered(_ list: [JSON]) -> [JSON] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return q.isEmpty ? list : list.filter { "\($0["name"].str) \($0["description"].str)".lowercased().contains(q) }
    }

    // MARK: Actions

    private func reload() async {
        async let a: Void = store.loadSkillSources()
        async let b: Void = store.loadSkills()
        _ = await (a, b)
    }

    private func add() {
        let url = link.trimmingCharacters(in: .whitespaces)
        guard !url.isEmpty, !adding else { return }
        adding = true
        Task {
            defer { adding = false }
            do {
                let s = try await store.api.post("/api/skill-sources", ["url": JSON(url)], timeout: 240)
                link = ""
                open.insert(s["id"].str)
                await reload()
                store.show("已添加 \(s["repo"].str)：找到 \(s["skills"].array.count) 个 Skill")
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func update(_ id: String) {
        Task { await store.updateSkillSources([id]) }
    }

    private func remove(_ s: JSON) {
        guard confirm("删除来源 \(s["repo"].str)？", "本机的仓库副本会删除，已经开始的 Run 不受影响。", ok: "删除") else { return }
        Task {
            do {
                try await store.api.delete("/api/skill-sources/\(s["id"].str)")
                await reload()
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func deleteOwn(_ k: JSON) {
        guard confirm("删除 Skill \(k["name"].str)？", "会从 Janus Skill 库中删除，已经开始的 Run 不受影响。", ok: "删除") else { return }
        Task {
            do {
                try await store.api.delete("/api/skills/\(k["id"].str)")
                await reload()
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func importFolder() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.showsHiddenFiles = true
        p.prompt = "复制到 Skill 库"
        p.message = "选择包含 SKILL.md 的文件夹"
        guard p.runModal() == .OK, let u = p.url else { return }
        Task {
            do {
                let sk = try await store.api.post("/api/skills/import", ["path": JSON(u.path)])
                await reload()
                store.show("已导入 Skill \(sk["name"].str)")
            } catch { store.show(error.localizedDescription) }
        }
    }
}
