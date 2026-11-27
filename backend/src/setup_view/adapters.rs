//! What an Agent card's runtime line shows, worked out from the adapter as `GET /api/adapters`
//! lists it: the state word after the version, the version menu (resume automatic updates, switch
//! back), Janus's own notes about the last install for the failure popover, and the hover text.
//! Clients only lay these out; whether the person already saw an automatic update is theirs.
// @project-doc docs/architecture/20261031-adapters.md#api_shape

use crate::lang::{Lang, tr};
use serde_json::{Value, json};

/// How long "已自动更新" stays after the automatic job ended (ms).
const AUTO_UPDATED_FOR: f64 = 10.0 * 60.0 * 1000.0;

fn running(a: &Value) -> bool {
    a["job"]["state"] == "running"
}

fn has_current(a: &Value) -> bool {
    a["current"].as_str().is_some_and(|c| !c.is_empty())
}

/// The installed versions other than the current one, the version installed last first.
fn others(a: &Value) -> Vec<String> {
    let mut v: Vec<&Value> = a["versions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["current"] != true && v["version"].as_str().is_some_and(|s| !s.is_empty()))
        .collect();
    v.sort_by(|x, y| y["installedAt"].as_f64().unwrap_or(0.0).total_cmp(&x["installedAt"].as_f64().unwrap_or(0.0)));
    v.iter().map(|v| v["version"].as_str().unwrap_or("").to_string()).collect()
}

/// The state after the version: `idle`, `updating`, `installing`, `autoUpdated` (an automatic
/// update just finished), `failed`, `notInstalled` or `installFailed`.
fn state(a: &Value, now: f64) -> &'static str {
    if running(a) {
        return if has_current(a) { "updating" } else { "installing" };
    }
    if !has_current(a) {
        return if a["job"]["state"] == "error" { "installFailed" } else { "notInstalled" };
    }
    if a["job"]["state"] == "error" {
        return "failed";
    }
    if auto_updated_until(a).is_some_and(|until| now < until) {
        return "autoUpdated";
    }
    "idle"
}

/// When "已自动更新" stops showing, if an automatic update just finished.
fn auto_updated_until(a: &Value) -> Option<f64> {
    (a["job"]["action"] == "auto" && a["job"]["state"] == "ok")
        .then(|| a["job"]["ended"].as_f64().unwrap_or(0.0) + AUTO_UPDATED_FOR)
}

/// Janus's own notes about the last install: the log's `== <time> <note>` lines without the time,
/// newest last. The tool's command line and output stay in the log (`GET /api/adapters/{id}/log`);
/// a line naming the package or `node_modules` (an older log) is skipped too.
fn recent_notes(a: &Value, limit: usize) -> Vec<String> {
    const HIDDEN: [&str; 3] = ["npm", "@agentclientprotocol/", "node_modules"];
    let notes: Vec<String> = a["log"]
        .as_str()
        .unwrap_or("")
        .lines()
        .filter_map(|line| {
            let s = line.trim().strip_prefix("== ")?;
            let (_, note) = s.split_once(' ')?;
            let note = note.trim();
            (!note.is_empty() && !HIDDEN.iter().any(|h| note.to_lowercase().contains(h))).then(|| note.to_string())
        })
        .collect();
    notes[notes.len().saturating_sub(limit)..].to_vec()
}

/// The runtime line of one adapter (`line` on each adapter of `GET /api/adapters?lang=`).
pub fn line(a: &Value, now: f64, lang: Lang) -> Value {
    let state = state(a, now);
    let word = match state {
        "updating" => Some(lang.pick("更新中…", "Updating…")),
        "installing" => Some(lang.pick("安装中…", "Installing…")),
        "autoUpdated" => Some(lang.pick("已自动更新", "Auto-Updated")),
        "notInstalled" => Some(lang.pick("运行组件还没装", "Runtime Not Installed")),
        _ => None,
    };
    let failure = match state {
        "failed" => Some(lang.pick("更新没成功", "Update Didn't Finish")),
        "installFailed" => Some(lang.pick("安装没成功", "Installation Didn't Finish")),
        _ => None,
    };
    let mut menu = vec![];
    if a["pinned"] == true {
        let latest = a["latest"].as_str().unwrap_or("");
        let label = if latest.is_empty() {
            lang.pick("恢复自动更新", "Resume Auto-Updates").to_string()
        } else {
            tr!(lang, "恢复自动更新（{latest}）", "Resume Auto-Updates ({latest})")
        };
        menu.push(json!({ "action": "install", "label": label }));
    }
    for v in others(a) {
        menu.push(json!({ "action": "use", "version": v, "label": tr!(lang, "切回 {v}", "Switch to {v}") }));
    }
    let hover = if !has_current(a) {
        lang.pick(
            "第一次检查 Agent 时会自动装。装不上时可以在这里再来一次。",
            "It installs the first time you check the Agent. If that doesn't work, try again here.",
        )
    } else if a["pinned"] == true {
        lang.pick(
            "你选了这个版本，它不会被自动更新，直到你恢复自动更新。",
            "You picked this version, so it won't auto-update until you resume auto-updates.",
        )
    } else {
        lang.pick(
            "Janus 用它启动 Agent。它会自动保持最新，正在跑的会话不受影响。",
            "Janus uses it to start Agents. It keeps itself up to date; sessions already running keep their version.",
        )
    };
    let detail = a["job"]["detail"]
        .as_str()
        .filter(|d| !d.is_empty())
        .unwrap_or(lang.pick("没有更多信息", "Nothing more to show"));
    json!({
        "state": state,
        "word": word,
        "failure": failure,
        "running": running(a),
        // An action can be offered: install when nothing is, or the menu when it has something.
        "install": matches!(state, "notInstalled" | "installFailed") && menu.is_empty(),
        "menu": menu,
        "notes": recent_notes(a, 4),
        "detail": detail,
        "hover": hover,
        // Which automatic update this is and until when its word shows: a client that already
        // showed it once leaves it out.
        "autoUpdated": auto_updated_until(a).map(|until| json!({
            "key": format!("{}@{}", a["id"].as_str().unwrap_or(""), a["job"]["ended"].as_f64().unwrap_or(0.0) as i64),
            "until": until,
        })),
    })
}

/// The adapters as `GET /api/adapters` returns them, each with its `line` in `lang`.
pub fn dress(adapters: &mut Value, now: f64, lang: Lang) {
    for a in adapters["adapters"].as_array_mut().into_iter().flatten() {
        a["line"] = line(a, now, lang);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang::{En, Zh};

    fn adapter(job: Value) -> Value {
        json!({ "id": "codex", "current": "1.2.0", "latest": "1.3.0", "job": job, "versions": [
            { "version": "1.2.0", "current": true, "installedAt": 3 },
            { "version": "1.0.0", "installedAt": 1 },
            { "version": "1.1.0", "installedAt": 2 },
        ] })
    }

    #[test]
    fn a_runtime_line_says_how_its_version_stands() {
        let now = 1_000_000.0;
        assert_eq!(state(&adapter(json!({ "state": "running" })), now), "updating");
        assert_eq!(state(&json!({ "job": { "state": "running" } }), now), "installing");
        assert_eq!(state(&json!({ "job": { "state": "error" } }), now), "installFailed");
        assert_eq!(state(&json!({}), now), "notInstalled");
        assert_eq!(state(&adapter(json!({ "state": "error" })), now), "failed");
        let auto = adapter(json!({ "action": "auto", "state": "ok", "ended": now - 1000.0 }));
        assert_eq!(state(&auto, now), "autoUpdated");
        assert_eq!(state(&auto, now + AUTO_UPDATED_FOR), "idle", "the word goes after a while");
        let l = line(&auto, now, En);
        assert_eq!(l["word"], "Auto-Updated");
        assert_eq!(l["autoUpdated"]["key"], format!("codex@{}", (now - 1000.0) as i64));
        assert_eq!(state(&adapter(json!({ "action": "install", "state": "ok" })), now), "idle");
    }

    #[test]
    fn the_version_menu_resumes_updates_then_switches_back() {
        let mut a = adapter(json!({ "state": "ok" }));
        a["pinned"] = json!(true);
        let l = line(&a, 0.0, Zh);
        let labels: Vec<&str> = l["menu"].as_array().unwrap().iter().map(|m| m["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["恢复自动更新（1.3.0）", "切回 1.1.0", "切回 1.0.0"]);
        assert_eq!(l["menu"][1]["version"], "1.1.0");
        assert_eq!(l["hover"], "你选了这个版本，它不会被自动更新，直到你恢复自动更新。");
        let missing = line(&json!({ "job": { "state": "error", "detail": "网络不通" } }), 0.0, En);
        assert_eq!(
            (missing["install"].as_bool(), missing["failure"].as_str()),
            (Some(true), Some("Installation Didn't Finish"))
        );
        assert_eq!(missing["detail"], "网络不通");
    }

    #[test]
    fn the_failure_notes_are_janus_own() {
        let a = json!({ "log": "== 10:00 开始安装 1.3.0\n$ npm install @agentclientprotocol/codex-acp\nadded 3 packages\n== 10:01 npm 退出码 1\n== 10:02 安装失败\n== 10:03 \n" });
        assert_eq!(recent_notes(&a, 4), ["开始安装 1.3.0", "安装失败"]);
        assert_eq!(recent_notes(&a, 1), ["安装失败"]);
    }
}
