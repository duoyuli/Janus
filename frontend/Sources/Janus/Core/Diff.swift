import Foundation

/// A unified diff as Janus saves it for a task (`git diff-index -p HEAD`, then a list of new
/// untracked files): files, hunks and lines with their numbers on each side, for the review view.
// @project-doc docs/architecture/frontend.md#diff_review
enum Diff {
    struct Line: Identifiable, Equatable {
        enum Kind { case context, added, removed }
        let id: Int
        let kind: Kind
        let text: String
        let old: Int?
        let new: Int?
    }
    struct Hunk: Identifiable, Equatable {
        let id: Int
        let header: String
        var lines: [Line] = []
    }
    struct File: Identifiable, Equatable {
        let id: Int
        var path: String
        var hunks: [Hunk] = []
        var added: Int { hunks.flatMap(\.lines).filter { $0.kind == .added }.count }
        var removed: Int { hunks.flatMap(\.lines).filter { $0.kind == .removed }.count }
        /// A new file Git does not track yet: no content in the diff, only its name.
        var untracked = false
    }

    static func parse(_ text: String) -> [File] {
        var files: [File] = []
        var oldN = 0, newN = 0, lineId = 0
        var inUntracked = false
        for raw in text.components(separatedBy: "\n") {
            let line = raw.hasSuffix("\r") ? String(raw.dropLast()) : raw
            if line == "# 未跟踪的新文件" { inUntracked = true; continue }
            if inUntracked {
                let p = line.trimmingCharacters(in: .whitespaces)
                if !p.isEmpty { files.append(File(id: files.count, path: p, untracked: true)) }
                continue
            }
            if line.hasPrefix("diff --git ") {
                // "diff --git a/x b/x": the b side is the file as it is now.
                let path = line.components(separatedBy: " b/").last ?? line
                files.append(File(id: files.count, path: path))
                continue
            }
            guard !files.isEmpty else { continue }
            if line.hasPrefix("+++ ") {
                let p = String(line.dropFirst(4))
                if p != "/dev/null" { files[files.count - 1].path = p.hasPrefix("b/") ? String(p.dropFirst(2)) : p }
                continue
            }
            if line.hasPrefix("--- ") || line.hasPrefix("index ") || line.hasPrefix("new file") || line.hasPrefix("deleted file") || line.hasPrefix("similarity") || line.hasPrefix("rename ") || line.hasPrefix("old mode") || line.hasPrefix("new mode") { continue }
            if line.hasPrefix("@@") {
                // @@ -a,b +c,d @@ context
                let nums = line.split(separator: " ").prefix(3)
                func start(_ s: Substring?) -> Int { Int(s?.dropFirst().split(separator: ",").first ?? "") ?? 0 }
                oldN = start(nums.dropFirst().first)
                newN = start(nums.dropFirst(2).first)
                let f = files.count - 1
                files[f].hunks.append(Hunk(id: files[f].hunks.count, header: line))
                continue
            }
            let f = files.count - 1
            guard !files[f].hunks.isEmpty else { continue }
            let h = files[f].hunks.count - 1
            lineId += 1
            switch line.first {
            case "+":
                files[f].hunks[h].lines.append(Line(id: lineId, kind: .added, text: String(line.dropFirst()), old: nil, new: newN)); newN += 1
            case "-":
                files[f].hunks[h].lines.append(Line(id: lineId, kind: .removed, text: String(line.dropFirst()), old: oldN, new: nil)); oldN += 1
            case " ":
                files[f].hunks[h].lines.append(Line(id: lineId, kind: .context, text: String(line.dropFirst()), old: oldN, new: newN)); oldN += 1; newN += 1
            default:
                continue // "\ No newline at end of file", trailing blank line
            }
        }
        return files
    }

    /// One comment on one line, as the person wrote it in the review.
    struct Comment: Equatable {
        let file: String
        let line: Line
        let text: String
    }

    /// The review as a message to an agent: where each comment is, the line it is about, and
    /// what to do. The agent reads the workspace for the rest.
    static func message(task: String, artifact: String, comments: [Comment], summary: String) -> String {
        var out = ["对 \(task) 的改动（\(artifact)）的审阅意见，请按这些意见修改，改完再逐条说明怎么处理的："]
        let s = summary.trimmingCharacters(in: .whitespacesAndNewlines)
        if !s.isEmpty { out.append("\n总体意见：\(s)") }
        for c in comments {
            let at = c.line.new.map { "第 \($0) 行" } ?? c.line.old.map { "原第 \($0) 行（已删除）" } ?? ""
            let mark = c.line.kind == .added ? "+" : c.line.kind == .removed ? "-" : " "
            out.append("\n- \(c.file) \(at)：`\(mark)\(c.line.text.trimmingCharacters(in: .whitespaces))`\n  \(c.text)")
        }
        return out.joined(separator: "\n")
    }
}
