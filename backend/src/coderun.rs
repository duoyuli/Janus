//! The MCP router's code mode (`run_code`): a script that calls the router's tools and gives
//! back only what it returns and logs. A model that needs several calls, or one call whose answer
//! it reads only in part, makes one call in place of many, and the answers it doesn't need never
//! reach its context.
//!
//! The script runs in QuickJS (`script`), with nothing of the computer's: no files, network,
//! processes, timers, `require` or `import`. It runs in a process of its own, Janus started again
//! as `janus run-code-helper` (`main`): the parent (`run`) gives it the script and limits on stdin
//! as JSON lines, runs each tool call it asks for on stdout, and answers on stdin. QuickJS stops a
//! script at its memory limit and at an interrupt between its steps; a builtin that runs long
//! between two steps is past the interrupt, so the parent ends the helper when it is past its
//! time, whatever it is doing.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

mod script;

use anyhow::{Result, anyhow, bail};
use script::Answer;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::future::Future;
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

/// The hidden subcommand that runs a script: Janus's own binary, started again by `run`.
pub const COMMAND: &str = "run-code-helper";

/// How long past its `wait` a helper gets before it is ended: a script in a builtin doesn't hear
/// the interrupt.
const GRACE: Duration = Duration::from_secs(2);

/// One script to run: `code`, the body of an async function, with `args` as its `args`, `tools`
/// the names on `tools` and `skills` the functions on `skills`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Script {
    pub code: String,
    pub args: Option<Value>,
    pub tools: Vec<String>,
    pub skills: Vec<Skill>,
}

/// A skill with code: its `code.js`, whose default export is an async function of one object of
/// args. A script calls it as `skills.<name>(args)` (`-` as `_`), and its code calls the same tools.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub code: String,
}

/// What one script may do.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Limits {
    /// How many tool calls it makes at most, and how many run at the same time.
    pub calls: usize,
    pub at_once: usize,
    /// How many bytes of what it returns and logs go back.
    pub max: usize,
    /// How much memory its runtime may take.
    pub heap: u64,
    /// How long it runs in all, its calls included.
    #[serde(with = "millis")]
    pub wait: Duration,
}

impl Limits {
    /// The router's: 100 calls, 8 at a time, 40000 bytes back, 512 MB, and 50 seconds, under the
    /// 60 an MCP client gives a call.
    pub const DEFAULT: Limits =
        Limits { calls: 100, at_once: 8, max: 40_000, heap: 512 << 20, wait: Duration::from_secs(50) };
}

/// A duration as whole milliseconds on the wire.
mod millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(d)?))
    }
}

/// What the parent gives the helper first.
#[derive(Serialize, Deserialize)]
struct Start {
    script: Script,
    limits: Limits,
}

/// One message from the helper: a tool call it wants run, or what the script came to.
#[derive(Deserialize)]
struct Said {
    call: Option<Call>,
    done: Option<Value>,
}

#[derive(Serialize, Deserialize)]
struct Call {
    id: u64,
    name: String,
    args: Value,
}

/// A tool result of text.
pub(crate) fn result(text: &str, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

/// A duration as the texts say it: `50s`, or `300ms` when it isn't whole seconds.
fn wait_text(d: Duration) -> String {
    if d.subsec_millis() == 0 { format!("{}s", d.as_secs()) } else { format!("{}ms", d.as_millis()) }
}

/// `s` cut to `most` bytes, saying how much more there was: `n` is its whole length when only
/// part of it was kept (the logs).
fn cut(s: &str, n: usize, most: usize) -> String {
    if s.len() <= most && n <= s.len() {
        return s.to_string();
    }
    let kept = &s[..s.floor_char_boundary(most.min(s.len()))];
    format!("{kept}\n… cut: {} more characters. Return only what you need.", n.saturating_sub(kept.len()))
}

/// A `run_code` result: what the script logged (`logged` bytes in all), then what it gave, each
/// cut to `most`.
fn out(logs: &str, logged: usize, said: &str, is_error: bool, most: usize) -> Value {
    let mut text = String::new();
    if !logs.is_empty() {
        text.push_str(&cut(logs, logged, most));
        if !said.is_empty() && !logs.ends_with('\n') {
            text.push('\n');
        }
    }
    text.push_str(&cut(said, said.len(), most));
    result(&text, is_error)
}

/// The tool result of what a script returned, after what it logged: nothing, a string as text,
/// MCP content blocks as content (an image a tool gave), anything else as JSON.
fn returned(logs: &str, logged: usize, kind: &str, said: &str, most: usize) -> Value {
    match kind {
        "none" if logs.is_empty() => {
            out("", 0, "The code returned nothing and logged nothing: return what you need.", false, most)
        }
        "none" => out(logs, logged, "", false, most),
        "string" => out(logs, logged, said, false, most),
        _ => {
            let parsed: Option<Value> = serde_json::from_str(said).ok();
            if let Some(Value::Array(parts)) = &parsed
                && !parts.is_empty()
                && parts.iter().all(is_content)
            {
                let mut content = vec![];
                if !logs.is_empty() {
                    content.push(json!({ "type": "text", "text": cut(logs, logged, most) }));
                }
                for p in parts {
                    let mut p = p.clone();
                    if p["type"] == "text" {
                        let t = p["text"].as_str().unwrap_or_default().to_string();
                        p["text"] = json!(cut(&t, t.len(), most));
                    }
                    content.push(p);
                }
                return json!({ "content": content });
            }
            let pretty = parsed.and_then(|v| serde_json::to_string_pretty(&v).ok()).filter(|p| p.len() <= most);
            out(logs, logged, pretty.as_deref().unwrap_or(said), false, most)
        }
    }
}

/// Whether `part` is an MCP content block, of a type MCP names.
fn is_content(part: &Value) -> bool {
    matches!(part["type"].as_str(), Some("text" | "image" | "audio" | "resource" | "resource_link"))
}

/// A skill's `code.js` as an expression whose value is its default export: the file run as a
/// function's body, with its `export default` made that function's return. An error when the file
/// has no `export default`, or another import or export.
pub fn skill_source(code: &str) -> Result<String> {
    let (start, end) =
        export_default(code).ok_or_else(|| anyhow!("its code.js has no `export default async (args) => {{ … }}`"))?;
    if other_statement(&code[..start]) || other_statement(&code[end..]) {
        bail!("its code.js imports or exports more than its default: it runs with nothing but tools and skills");
    }
    // const, not return: functions declared after it are hoisted to it
    Ok(format!(
        "(function () {{\n{}const __janusDefault = {}\n;return __janusDefault;\n}})()",
        &code[..start],
        &code[end..]
    ))
}

/// Where each line of `s` starts.
fn line_starts(s: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(s.match_indices('\n').map(|(i, _)| i + 1))
}

fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `s` from `i` past any spaces and tabs.
fn skip_blank(s: &str, i: usize) -> usize {
    i + s[i..].len() - s[i..].trim_start_matches([' ', '\t']).len()
}

/// Whether `s` at `i` is the word `w`, as a whole word; where it ends.
fn word_at(s: &str, i: usize, w: &str) -> Option<usize> {
    let end = i + w.len();
    (s[i..].starts_with(w) && !s[end..].starts_with(word_char)).then_some(end)
}

/// The first `export default` at a line's start, from that line's start to past the blanks after
/// `default`.
fn export_default(s: &str) -> Option<(usize, usize)> {
    line_starts(s).find_map(|start| {
        let i = word_at(s, skip_blank(s, start), "export")?;
        let j = skip_blank(s, i);
        if j == i {
            return None;
        }
        Some((start, skip_blank(s, word_at(s, j, "default")?)))
    })
}

/// Whether `s` has an import or export statement at a line's start.
fn other_statement(s: &str) -> bool {
    line_starts(s).any(|start| {
        let i = skip_blank(s, start);
        ["import", "export"].iter().filter_map(|w| word_at(s, i, w)).any(|end| {
            s[skip_blank(s, end)..].starts_with(|c: char| word_char(c) || matches!(c, '{' | '*' | '\'' | '"'))
        })
    })
}

/// Runs `script` in a helper process (`exe COMMAND`), each tool call it asks for run here by
/// `call(name, args)`, which gives the tool's MCP result or an error the script is thrown (no such
/// tool). At most `limits.at_once` calls run at once, and the helper is ended past its time and
/// `GRACE`. Never fails: whatever goes wrong comes back as a tool result that is an error. A call
/// still running when the script ends is dropped.
pub async fn run<F, Fut>(exe: &Path, script: Script, limits: Limits, call: F) -> Value
where
    F: Fn(String, Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value>> + Send + 'static,
{
    match tokio::time::timeout(limits.wait + GRACE, drive(exe, script, limits, Arc::new(call))).await {
        Ok(res) => res,
        Err(_) => result(&format!("The code was stopped: it didn't finish in {}.", wait_text(limits.wait)), true),
    }
}

async fn drive<F, Fut>(exe: &Path, script: Script, limits: Limits, call: Arc<F>) -> Value
where
    F: Fn(String, Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value>> + Send + 'static,
{
    let child = tokio::process::Command::new(exe)
        .arg(COMMAND)
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return result(&format!("code mode couldn't start: {e}"), true),
    };
    let (Some(stdin), Some(stdout), Some(mut stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return result("code mode couldn't start: no pipes to its runner", true);
    };
    // The last few KB the helper wrote to stderr: why it quit.
    let errs = tokio::spawn(async move {
        let mut all = vec![];
        let _ = stderr.read_to_end(&mut all).await;
        String::from_utf8_lossy(&all[all.len().saturating_sub(4096)..]).trim().to_string()
    });
    let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
    let send = |stdin: Arc<tokio::sync::Mutex<tokio::process::ChildStdin>>, v: Value| async move {
        let mut line = v.to_string();
        line.push('\n');
        let _ = stdin.lock().await.write_all(line.as_bytes()).await;
    };
    let start = Start { script, limits };
    send(stdin.clone(), serde_json::to_value(&start).unwrap_or_default()).await;

    let slots = Arc::new(tokio::sync::Semaphore::new(limits.at_once.max(1)));
    let mut calls = tokio::task::JoinSet::new();
    let mut made = 0usize;
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    loop {
        let Ok(Some(line)) = lines.next_line().await else { break };
        let Ok(said) = serde_json::from_str::<Said>(&line) else { continue };
        if let Some(done) = said.done {
            // It exits once it has answered; reap it here, and end it if it lingers.
            drop(stdin);
            if tokio::time::timeout(GRACE, child.wait()).await.is_err() {
                let _ = child.kill().await;
            }
            return done;
        }
        let Some(c) = said.call else { continue };
        made += 1;
        if made > limits.calls {
            let a = Answer {
                id: c.id,
                res: None,
                err: Some(format!("more than {} tool calls in one script", limits.calls)),
            };
            send(stdin.clone(), serde_json::to_value(a).unwrap_or_default()).await;
            continue;
        }
        let (call, slots, stdin) = (call.clone(), slots.clone(), stdin.clone());
        calls.spawn(async move {
            let Ok(_slot) = slots.acquire_owned().await else { return };
            let a = match call(c.name, c.args).await {
                Ok(res) => Answer { id: c.id, res: Some(res), err: None },
                Err(e) => Answer { id: c.id, res: None, err: Some(format!("{e:#}")) },
            };
            send(stdin, serde_json::to_value(a).unwrap_or_default()).await;
        });
    }
    // The helper quit without an answer.
    drop(stdin);
    let _ = child.wait().await;
    let errs = errs.await.unwrap_or_default();
    let why = if errs.contains("out of memory") {
        "it ran out of memory".to_string()
    } else if let Some(last) = errs.lines().last() {
        format!("its runner quit: {last}")
    } else {
        "its runner quit".to_string()
    };
    result(&format!("The code was stopped: {why}."), true)
}

/// The helper (`janus run-code-helper`): runs the script the parent gives it on stdin, asking the
/// parent for each tool call on stdout, and writes what it came to. Its exit code is 0 once it has
/// answered, 2 when it was given no script, 1 when the parent went away first.
pub fn main() -> i32 {
    let mut input = std::io::BufReader::new(std::io::stdin());
    let mut first = String::new();
    let start: Start = match input
        .read_line(&mut first)
        .map_err(anyhow::Error::from)
        .and_then(|_| Ok(serde_json::from_str(&first)?))
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("janus {COMMAND}: {e:#}");
            return 2;
        }
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in input.lines() {
            let Ok(line) = line else { break };
            if let Ok(a) = serde_json::from_str::<Answer>(&line)
                && tx.send(a).is_err()
            {
                return;
            }
        }
        // The parent is gone, or ended the script: nothing is left to answer to.
        std::process::exit(1);
    });
    let mut io = Stdio { answers: rx };
    let res = script::run(&start.script, &start.limits, &mut io);
    write_line(&json!({ "done": res }));
    0
}

/// The helper's end of the pipes: calls out on stdout, answers in from the reading thread.
struct Stdio {
    answers: std::sync::mpsc::Receiver<Answer>,
}

impl script::Io for Stdio {
    fn call(&mut self, id: u64, name: &str, args: Value) {
        write_line(&json!({ "call": Call { id, name: name.to_string(), args } }));
    }

    fn answer(&mut self, until: Instant) -> Option<Answer> {
        self.answers.recv_timeout(until.saturating_duration_since(Instant::now())).ok()
    }
}

fn write_line(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

/// The `janus` binary next to this test binary (`target/debug/janus`), which `cargo test` builds
/// for the integration tests: the helper for tests that run a script end to end.
#[cfg(test)]
pub fn test_exe() -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("the test binary's path");
    let janus = exe.parent().and_then(Path::parent).expect("target/debug").join("janus");
    assert!(janus.exists(), "{} is not built: run the whole `cargo test`, which builds it", janus.display());
    janus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_is_its_default_export() {
        let src = skill_source("function helper() { return 1 }\n  export default async (args) => helper()").unwrap();
        assert!(src.contains("const __janusDefault = async (args) => helper()"), "{src}");
        assert!(src.starts_with("(function () {\nfunction helper()"), "{src}");
        assert!(skill_source("export const x = 1;").is_err(), "export without default");
        assert!(skill_source("// export default in a comment\nconst x = 1;").is_err());
        assert!(skill_source("export default 1;\nexport { x };").is_err(), "a second export");
        assert!(skill_source("import { a } from 'b';\nexport default 1;").is_err(), "an import");
        assert!(
            skill_source("const s = 'import x';\nexport default async () => import_count;").is_ok(),
            "not at a line start"
        );
        assert!(skill_source("exportdefault 1").is_err());
    }

    #[test]
    fn a_cut_says_how_much_more_there_was() {
        assert_eq!(cut("abc", 3, 10), "abc");
        assert_eq!(cut("abcdef", 6, 4), "abcd\n… cut: 2 more characters. Return only what you need.");
        assert_eq!(
            cut("截图截图", 12, 4),
            "截\n… cut: 9 more characters. Return only what you need.",
            "never inside a character"
        );
    }

    /// The helper end to end: the real `janus run-code-helper`, its calls run here, at most
    /// `at_once` at a time.
    #[tokio::test]
    async fn runs_a_script_in_the_helper() {
        let running = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let most = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (r, m) = (running.clone(), most.clone());
        let script = Script {
            code: "const rs = await Promise.all([1, 2, 3, 4, 5].map((n) => tools.math__double({n}))); console.log('sum'); return rs.reduce((a, b) => a + Number(b), 0);".into(),
            args: None,
            tools: vec!["math__double".into()],
            skills: vec![],
        };
        let limits = Limits { at_once: 2, ..Limits::DEFAULT };
        let res = run(&test_exe(), script, limits, move |name, args| {
            let (r, m) = (r.clone(), m.clone());
            async move {
                assert_eq!(name, "math__double");
                let now = r.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                m.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                r.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                Ok(json!({ "content": [{ "type": "text", "text": (args["n"].as_i64().unwrap() * 2).to_string() }] }))
            }
        })
        .await;
        assert_eq!(res["isError"], false, "{res}");
        assert_eq!(res["content"][0]["text"], "sum\n30");
        assert_eq!(most.load(std::sync::atomic::Ordering::SeqCst), 2, "at most at_once calls at a time");

        // An unknown tool's error is the script's to catch.
        let script = Script { code: "await tools.nope();".into(), args: None, tools: vec![], skills: vec![] };
        let res =
            run(
                &test_exe(),
                script,
                Limits::DEFAULT,
                |name, _| async move { Err(anyhow!("no tool is called {name}")) },
            )
            .await;
        assert!(res["content"][0]["text"].as_str().unwrap().contains("no tool is called nope"), "{res}");
    }

    /// A helper that can't be started is said as the result.
    #[tokio::test]
    async fn a_missing_helper_is_said() {
        let script = Script { code: "return 1;".into(), args: None, tools: vec![], skills: vec![] };
        let res = run(Path::new("/nonexistent/janus"), script, Limits::DEFAULT, |_, _| async { Ok(json!({})) }).await;
        assert_eq!(res["isError"], true);
        assert!(res["content"][0]["text"].as_str().unwrap().starts_with("code mode couldn't start"), "{res}");
    }
}
