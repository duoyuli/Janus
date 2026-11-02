import Foundation

/// The Markdown files of a run, for the 文档 menu: what the lead delivered, the artifacts Janus
/// saved, and files the agents wrote or mentioned (absolute paths, or paths relative to the
/// run's directory). Scanning only collects candidates; the menu shows those that exist.
// @project-doc docs/architecture/frontend.md#markdown_viewer
enum RunDocs {
    enum Source: Int, Comparable {
        case deliverable, artifact, mentioned
        static func < (a: Source, b: Source) -> Bool { a.rawValue < b.rawValue }
    }

    struct Doc: Hashable {
        let path: String
        var source: Source
        /// The latest event that delivered, saved or mentioned it.
        var seq: Int
    }

    /// Adds the candidates in `events` to `found`, keyed by path; a path keeps its strongest
    /// source and its latest mention.
    static func scan<S: Sequence>(_ events: S, cwd: String, into found: inout [String: Doc]) where S.Element == Event {
        func add(_ path: String, _ source: Source, _ seq: Int) {
            guard isMarkdown(path) else { return }
            let p = normalize(path, cwd: cwd)
            if var d = found[p] {
                d.source = min(d.source, source)
                d.seq = max(d.seq, seq)
                found[p] = d
            } else {
                found[p] = Doc(path: p, source: source, seq: seq)
            }
        }
        for e in events {
            switch e.type {
            case "run.delivered":
                for a in e.data["deliverables"].array { add(a["path"].str, .deliverable, e.seq) }
            case "artifact.created":
                add(e.data["path"].str, .artifact, e.seq)
                continue
            // Janus's own prompts quote instructions, not files the team produced; status lines repeat tool calls.
            case "session.prompt", "agent.status":
                continue
            default:
                break
            }
            for s in strings(e.data) + [e.text] { for p in paths(in: s) { add(p, .mentioned, e.seq) } }
        }
    }

    /// The candidates that exist, deliverables first, then artifacts, then mentions; newest first in each.
    static func existing(_ found: [String: Doc], exists: (String) -> Bool = { FileManager.default.fileExists(atPath: $0) }) -> [Doc] {
        found.values.filter { exists($0.path) }.sorted { ($0.source, -$0.seq, $0.path) < ($1.source, -$1.seq, $1.path) }
    }

    static func isMarkdown(_ path: String) -> Bool {
        ["md", "markdown"].contains((path as NSString).pathExtension.lowercased())
    }

    /// Markdown paths written in a piece of text. A relative name glued to Chinese text
    /// ("生成README.md") also yields the name without the Chinese prefix, and "a.md/b.md" yields
    /// each name; the existence check later keeps the real one.
    static func paths(in text: String) -> [String] {
        guard text.contains(".md") || text.contains(".markdown") else { return [] }
        var out: [String] = []
        var rest = text
        let stop = #"\s"'`<>()\[\]{}|,;，。；：、（）「」！？*"#
        let absolute = #"(?<![A-Za-z0-9_.\-/~])(?:~|/)[^\#(stop)]*?\.(?:md|markdown)(?![A-Za-z0-9_])"#
        for m in matches(absolute, in: text) {
            out.append(m)
            rest = rest.replacingOccurrences(of: m, with: " ")
        }
        let relative = #"(?<![A-Za-z0-9_.\-/~])[\p{L}\p{N}_.\-]+(?:/[\p{L}\p{N}_.\-]+)*\.(?:md|markdown)(?![A-Za-z0-9_])"#
        for m in matches(relative, in: rest) {
            out.append(m)
            if let r = m.range(of: #"[A-Za-z0-9_.\-][A-Za-z0-9_.\-/]*$"#, options: .regularExpression), r.lowerBound != m.startIndex { out.append(String(m[r])) }
            let parts = m.split(separator: "/").map(String.init).filter(isMarkdown)
            if parts.count > 1 { out += parts }
        }
        return out
    }

    static func normalize(_ path: String, cwd: String) -> String {
        var p = path
        if p.hasPrefix("~/") { p = NSHomeDirectory() + p.dropFirst(1) }
        if !p.hasPrefix("/"), !cwd.isEmpty { p = (cwd as NSString).appendingPathComponent(p) }
        return (p as NSString).standardizingPath
    }

    private static func strings(_ j: JSON) -> [String] {
        switch j {
        case .string(let s): return [s]
        case .array(let a): return a.flatMap(strings)
        case .object(let o): return o.values.flatMap(strings)
        default: return []
        }
    }

    private static func matches(_ pattern: String, in s: String) -> [String] {
        guard let re = try? NSRegularExpression(pattern: pattern) else { return [] }
        return re.matches(in: s, range: NSRange(s.startIndex..., in: s)).compactMap { Range($0.range, in: s).map { String(s[$0]) } }
    }
}
