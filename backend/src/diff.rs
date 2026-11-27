//! Changes as the person reads them: a task's saved diff (`git diff-index -p HEAD`, then a list
//! of new untracked files) file by file, hunk by hunk, with each line's number on both sides; the
//! lines a tool call's edit removed and added, with three lines of context; and a review of a diff
//! written back to an agent as one message. Worked out here once for every client
//! (docs/decisions/20261126-backend_owns_derived_state.md).
// @project-doc docs/architecture/20261031-frontend.md#diff_review

use crate::lang::{Lang, tr};
use serde::Serialize;

crate::wire::wire_enum! {
    /// What a line of a diff is.
    pub enum LineKind {
        Context = "context",
        Added = "added",
        Removed = "removed",
    }
}

/// One line of a hunk. `id` counts the lines of the whole diff, so a comment names one line.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Line {
    pub id: usize,
    pub kind: LineKind,
    pub text: String,
    /// Its number before the change and after it; none on the side it is not on.
    pub old: Option<usize>,
    pub new: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hunk {
    pub id: usize,
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct File {
    pub id: usize,
    pub path: String,
    pub hunks: Vec<Hunk>,
    pub added: usize,
    pub removed: usize,
    /// A new file Git does not track yet. Its diff is included when Git could read it; otherwise
    /// the artifact lists its name alone.
    pub untracked: bool,
}

impl File {
    fn new(id: usize, path: String) -> File {
        File { id, path, hunks: vec![], added: 0, removed: 0, untracked: false }
    }
}

/// The heading of the list of untracked files Janus adds after Git's diff.
const UNTRACKED: &str = "# 未跟踪的新文件";

/// A saved diff, file by file.
pub fn parse(text: &str) -> Vec<File> {
    let mut files: Vec<File> = vec![];
    let (mut old_n, mut new_n, mut line_id) = (0usize, 0usize, 0usize);
    let mut in_untracked = false;
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line == UNTRACKED {
            in_untracked = true;
            continue;
        }
        if in_untracked {
            let p = line.trim_matches(' ');
            if p.is_empty() {
                continue;
            }
            // The file's diff is above this list when Git could read it: mark it instead of
            // adding it twice.
            match files.iter_mut().find(|f| f.path == p) {
                Some(f) => f.untracked = true,
                None => {
                    let mut f = File::new(files.len(), p.to_string());
                    f.untracked = true;
                    files.push(f);
                }
            }
            continue;
        }
        if line.starts_with("diff --git ") {
            // "diff --git a/x b/x": the b side is the file as it is now.
            let path = line.rsplit(" b/").next().unwrap_or(line).to_string();
            files.push(File::new(files.len(), path));
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if let Some(p) = line.strip_prefix("+++ ") {
            if p != "/dev/null" {
                f.path = p.strip_prefix("b/").unwrap_or(p).to_string();
            }
            continue;
        }
        const HEADERS: &[&str] =
            &["--- ", "index ", "new file", "deleted file", "similarity", "rename ", "old mode", "new mode"];
        if HEADERS.iter().any(|h| line.starts_with(h)) {
            continue;
        }
        if line.starts_with("@@") {
            // @@ -a,b +c,d @@ context
            let nums: Vec<&str> = line.split(' ').filter(|s| !s.is_empty()).take(3).collect();
            let start = |s: Option<&&str>| -> usize {
                s.and_then(|s| s.get(1..)).and_then(|s| s.split(',').next()).and_then(|n| n.parse().ok()).unwrap_or(0)
            };
            old_n = start(nums.get(1));
            new_n = start(nums.get(2));
            f.hunks.push(Hunk { id: f.hunks.len(), header: line.to_string(), lines: vec![] });
            continue;
        }
        let Some(h) = f.hunks.last_mut() else { continue };
        let (kind, old, new) = match line.chars().next() {
            Some('+') => (LineKind::Added, None, Some(new_n)),
            Some('-') => (LineKind::Removed, Some(old_n), None),
            Some(' ') => (LineKind::Context, Some(old_n), Some(new_n)),
            // "\ No newline at end of file", a trailing blank line.
            _ => continue,
        };
        line_id += 1;
        old_n += usize::from(old.is_some());
        new_n += usize::from(new.is_some());
        match kind {
            LineKind::Added => f.added += 1,
            LineKind::Removed => f.removed += 1,
            LineKind::Context => {}
        }
        h.lines.push(Line { id: line_id, kind, text: line[1..].to_string(), old, new });
    }
    files
}

/// One line of an edit as it reads beside its neighbours: `+` added, `-` removed, ` ` kept.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Mark {
    pub mark: &'static str,
    pub text: String,
}

/// The most lines an edit shows.
const SHOWN_LINES: usize = 400;

/// What an edit from `old` to `new` changed: the lines removed and added, each with three lines
/// of context around it, the rest left out.
pub fn inline(old: &str, new: &str) -> Vec<Mark> {
    let a: Vec<&str> = old.split('\n').collect();
    let b: Vec<&str> = new.split('\n').collect();
    let (removed, inserted) = changes(&a, &b);
    let mut out: Vec<(&'static str, &str)> = vec![];
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && removed[i] {
            out.push(("-", a[i]));
            i += 1;
        } else if j < b.len() && inserted[j] {
            out.push(("+", b[j]));
            j += 1;
        } else {
            if j < b.len() {
                out.push((" ", b[j]));
            }
            i += 1;
            j += 1;
        }
    }
    let changed: Vec<usize> = (0..out.len()).filter(|&k| out[k].0 != " ").collect();
    (0..out.len())
        .filter(|&k| changed.iter().any(|&c| c.abs_diff(k) <= 3))
        .take(SHOWN_LINES)
        .map(|k| Mark { mark: out[k].0, text: out[k].1.to_string() })
        .collect()
}

/// Lines added and removed from `old` to `new`.
pub fn line_counts(old: &str, new: &str) -> (usize, usize) {
    fn lines(s: &str) -> Vec<&str> {
        if s.is_empty() { vec![] } else { s.split('\n').collect() }
    }
    let (a, b) = (lines(old), lines(new));
    let (removed, inserted) = changes(&a, &b);
    (inserted.iter().filter(|&&x| x).count(), removed.iter().filter(|&&x| x).count())
}

/// Beyond this many differences the middle of two texts is shown as replaced whole: finding the
/// shortest edit costs the square of it.
const MAX_EDIT: usize = 4000;

/// The shortest edit from `a` to `b` (Myers): which lines of `a` it removes and which of `b` it
/// inserts. The common start and end are taken off first.
fn changes(a: &[&str], b: &[&str]) -> (Vec<bool>, Vec<bool>) {
    let mut removed = vec![false; a.len()];
    let mut inserted = vec![false; b.len()];
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (a2, b2) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    match shortest_edit(a2, b2) {
        Some((r, i)) => {
            for x in r {
                removed[head + x] = true;
            }
            for y in i {
                inserted[head + y] = true;
            }
        }
        None => {
            removed[head..a.len() - tail].fill(true);
            inserted[head..b.len() - tail].fill(true);
        }
    }
    (removed, inserted)
}

/// Myers' greedy shortest edit script, as the indices removed from `a` and inserted from `b`;
/// none when it would take more than `MAX_EDIT` edits.
fn shortest_edit(a: &[&str], b: &[&str]) -> Option<(Vec<usize>, Vec<usize>)> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    if n == 0 || m == 0 {
        return Some(((0..a.len()).collect(), (0..b.len()).collect()));
    }
    let max = (n + m) as usize;
    let off = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    // The furthest x on each diagonal after each number of edits, for the way back.
    let mut trace: Vec<Vec<isize>> = vec![];
    for d in 0..=max.min(MAX_EDIT) as isize {
        trace.push(v[(off - d) as usize..=(off + d) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let ki = (off + k) as usize;
            let mut x = if k == -d || (k != d && v[ki - 1] < v[ki + 1]) { v[ki + 1] } else { v[ki - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[ki] = x;
            if x >= n && y >= m {
                return Some(back(&trace, n, m));
            }
            k += 2;
        }
    }
    None
}

/// Walk the edits back from the end. `trace[d]` holds each diagonal's furthest x before edit `d`,
/// indexed from diagonal `-d`.
fn back(trace: &[Vec<isize>], n: isize, m: isize) -> (Vec<usize>, Vec<usize>) {
    let (mut removed, mut inserted) = (vec![], vec![]);
    let (mut x, mut y) = (n, m);
    for d in (1..trace.len() as isize).rev() {
        let v = &trace[d as usize];
        let at = |k: isize| v[(k + d) as usize];
        let k = x - y;
        let down = k == -d || (k != d && at(k - 1) < at(k + 1));
        let pk = if down { k + 1 } else { k - 1 };
        let px = at(pk);
        let py = px - pk;
        if down {
            inserted.push(py as usize);
        } else {
            removed.push(px as usize);
        }
        (x, y) = (px, py);
    }
    removed.reverse();
    inserted.reverse();
    (removed, inserted)
}

/// One comment of a review: the line it is about, by its id in the parsed diff, and what to do.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Comment {
    pub line: usize,
    pub text: String,
}

/// A review of a diff as one message to an agent, in `lang`: what it is about, then where each
/// comment is, the line it is on, and what to do. The agent reads the workspace for the rest.
/// `subject` is the changes' owner: `None` for the whole run, else the task. Comments on lines the
/// diff does not have are left out.
pub fn message(
    lang: Lang,
    subject: Option<&str>,
    artifact: &str,
    files: &[File],
    comments: &[Comment],
    summary: &str,
) -> String {
    let what = match subject {
        None => lang.pick("整个任务的改动", "the whole run's changes").to_string(),
        Some(task) => tr!(lang, "{task} 的改动", "the changes of {task}"),
    };
    let mut out = vec![tr!(
        lang,
        "对{what}（{artifact}）的审阅意见，请按这些意见修改，改完再逐条说明怎么处理的：",
        "Review comments on {what} ({artifact}). Make these changes, then say how you handled each one:"
    )];
    let s = summary.trim();
    if !s.is_empty() {
        out.push(tr!(lang, "\n总体意见：{s}", "\nOverall: {s}"));
    }
    // As the review lists them: by file, then by line.
    let mut found: Vec<(&str, &Line, &str)> = files
        .iter()
        .flat_map(|f| f.hunks.iter().flat_map(|h| &h.lines).map(move |l| (f.path.as_str(), l)))
        .flat_map(|(path, l)| comments.iter().filter(move |c| c.line == l.id).map(move |c| (path, l, c.text.as_str())))
        .collect();
    found.sort_by(|a, b| (a.0, a.1.id).cmp(&(b.0, b.1.id)));
    let sep = lang.pick("：", ": ");
    for (path, l, text) in found {
        let at = match (l.new, l.old) {
            (Some(n), _) => tr!(lang, "第 {n} 行", "line {n}"),
            (None, Some(o)) => tr!(lang, "原第 {o} 行（已删除）", "former line {o} (removed)"),
            _ => String::new(),
        };
        let mark = match l.kind {
            LineKind::Added => "+",
            LineKind::Removed => "-",
            LineKind::Context => " ",
        };
        let line = l.text.trim_matches([' ', '\t']);
        out.push(format!("\n- {path} {at}{sep}`{mark}{line}`\n  {text}"));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests;
