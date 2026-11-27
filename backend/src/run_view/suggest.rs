//! What the composer offers while the person types: the agent's slash commands after `/` at the
//! start, ranked as Lody ranks them, and workspace files after `@` as the last word; each with
//! the draft it leaves when picked, and the arguments a picked command still takes.
// @project-doc docs/architecture/20261031-frontend.md#session_view

use super::aside::Command;
use crate::lang::{Lang, tr};
use serde::Serialize;

/// The word being typed at the end of a draft, when it asks for a suggestion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asking {
    /// `/re` at the start, no space yet.
    Command(String),
    /// `@sr` as the last word.
    File(String),
}

pub fn asking(draft: &str) -> Option<Asking> {
    if let Some(name) = draft.strip_prefix('/')
        && !draft.chars().any(char::is_whitespace)
    {
        return Some(Asking::Command(name.to_string()));
    }
    let last = draft.split(char::is_whitespace).next_back().unwrap_or("");
    last.strip_prefix('@').map(|f| Asking::File(f.to_string()))
}

/// The draft with the word being typed replaced by the pick, and a space to go on typing.
pub fn complete(draft: &str, pick: &Asking) -> String {
    match pick {
        Asking::Command(name) => format!("/{name} "),
        Asking::File(path) => {
            let cut = draft.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8());
            format!("{}@{path} ", &draft[..cut])
        }
    }
}

/// The commands matching what follows `/`, best first, in Lody's tiers: the name itself, a prefix
/// of it, a prefix of one of its words (after `-` `_` `/` `:` `.`), anywhere in it, its letters in
/// order, and last only the description. Within a tier the earlier and the shorter match wins.
/// Nothing typed yet keeps the agent's order, its own commands first.
pub fn rank<'a>(commands: &'a [Command], query: &str) -> Vec<&'a Command> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return commands.iter().filter(|c| !c.skill).chain(commands.iter().filter(|c| c.skill)).collect();
    }
    let score = |c: &Command| -> Option<(u8, usize, usize)> {
        let n = c.name.to_lowercase();
        let len = n.chars().count();
        if n == q {
            return Some((0, 0, len));
        }
        if n.starts_with(&q) {
            return Some((1, 0, len));
        }
        let words: Vec<&str> = n.split(|ch: char| "-_/:. ".contains(ch)).filter(|w| !w.is_empty()).collect();
        if let Some(w) = words.iter().position(|w| w.starts_with(&q)) {
            return Some((2, w, len));
        }
        if let Some(at) = n.find(&q) {
            return Some((3, n[..at].chars().count(), len));
        }
        let mut rest = q.chars().peekable();
        for ch in n.chars() {
            if rest.peek() == Some(&ch) {
                rest.next();
            }
        }
        if rest.peek().is_none() {
            return Some((4, 0, len));
        }
        if c.description.to_lowercase().contains(&q) {
            return Some((5, 0, len));
        }
        None
    };
    let mut scored: Vec<(&Command, (u8, usize, usize))> =
        commands.iter().filter_map(|c| score(c).map(|s| (c, s))).collect();
    scored.sort_by_key(|(_, s)| *s);
    scored.into_iter().map(|(c, _)| c).collect()
}

/// The arguments still to write after a command picked from the list: its hint, shown grey after
/// `/name ` until the person types on.
pub fn argument_hint(draft: &str, commands: &[Command]) -> Option<String> {
    let name = draft.strip_prefix('/')?.strip_suffix(' ')?;
    if draft.matches(' ').count() != 1 {
        return None;
    }
    commands.iter().find(|c| c.name == name && !c.hint.is_empty()).map(|c| c.hint.clone())
}

/// One suggestion.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Suggestion {
    /// `/review`, `@src/main.rs`.
    pub label: String,
    pub detail: String,
    /// The heading above this row, when it starts a group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The draft once this row is picked.
    pub completion: String,
}

/// What the composer offers for a draft.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Suggestions {
    pub rows: Vec<Suggestion>,
    /// The arguments a command picked from the list still takes.
    pub ghost: Option<String>,
}

/// The suggestions for `draft` given the agent's `commands`; `files` finds the workspace files
/// whose path contains what follows `@`, best first.
pub fn suggestions(commands: &[Command], draft: &str, l: Lang, files: impl FnOnce(&str) -> Vec<String>) -> Suggestions {
    let ghost = argument_hint(draft, commands);
    let rows = match asking(draft) {
        Some(Asking::Command(q)) => {
            let headed = q.is_empty() && commands.iter().any(|c| c.skill);
            let mut last: Option<bool> = None;
            rank(commands, &q)
                .into_iter()
                .take(50)
                .map(|c| {
                    let mut detail = vec![c.description.clone()];
                    if !c.hint.is_empty() {
                        detail.push(tr!(l, "参数：{}", "Parameters: {}", c.hint));
                    }
                    detail.retain(|d| !d.is_empty());
                    // Headings only for the whole list: once something is typed, the best match leads.
                    let group = (headed && last != Some(c.skill))
                        .then(|| {
                            if c.skill { l.pick("Skill", "Skills") } else { l.pick("Agent 命令", "Agent commands") }
                        })
                        .map(str::to_string);
                    last = Some(c.skill);
                    Suggestion {
                        label: format!("/{}", c.name),
                        detail: detail.join(" · "),
                        group,
                        completion: complete(draft, &Asking::Command(c.name.clone())),
                    }
                })
                .collect()
        }
        Some(Asking::File(q)) => files(&q)
            .into_iter()
            .take(8)
            .map(|f| Suggestion {
                label: format!("@{f}"),
                detail: String::new(),
                group: None,
                completion: complete(draft, &Asking::File(f)),
            })
            .collect(),
        None => vec![],
    };
    Suggestions { rows, ghost }
}
