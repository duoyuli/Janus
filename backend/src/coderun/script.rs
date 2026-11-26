//! One script, run in this process by QuickJS: only the helper (`coderun::main`) runs it, so
//! whatever the script does to memory or time stays in that process. QuickJS has its own memory
//! limit and stops at an interrupt between its steps; the parent still ends the helper past its
//! time, for a builtin that runs long between two steps.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

use super::{Limits, Script, out, result, returned, skill_source, wait_text};
use rquickjs::{Context, Function, Object, Persistent, Runtime};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

/// What the script finds before its code: `tools`, `skills`, `args`, `console`.
const PRELUDE: &str = include_str!("prelude.js");

/// How deep the script's native stack may grow: QuickJS throws past it.
const STACK: usize = 512 * 1024;

/// One tool call's answer, from whoever runs the tools.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct Answer {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub res: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub err: Option<String>,
}

/// Where a script's tool calls go: `call` asks for one, `answer` waits until `until` for the next
/// answer to any of them (none: the time is up).
pub(super) trait Io {
    fn call(&mut self, id: u64, name: &str, args: Value);
    fn answer(&mut self, until: Instant) -> Option<Answer>;
}

/// What the script's code came to.
enum Outcome {
    /// It returned: nothing, a string, or anything else as `show` gave it.
    Returned(&'static str, String),
    /// It threw, as `errorText` gave it.
    Threw(String),
}

/// What the host functions collect while the script runs.
#[derive(Default)]
struct State {
    outbox: Vec<(u64, String, String)>,
    logs: String,
    logged: usize,
    outcome: Option<Outcome>,
}

/// Runs `sc` within `lim`, each tool call it makes asked of `io`: the tool result of what it
/// returned and logged, or of why it failed or was stopped.
pub(super) fn run(sc: &Script, lim: &Limits, io: &mut dyn Io) -> Value {
    let args = match &sc.args {
        None | Some(Value::Null) => json!({}),
        Some(v @ Value::Object(_)) => v.clone(),
        Some(_) => return result("args isn't a JSON object", true),
    };
    let deadline = Instant::now() + lim.wait;
    let Ok(rt) = Runtime::new() else { return result("code mode couldn't start: no runtime", true) };
    rt.set_memory_limit(usize::try_from(lim.heap).unwrap_or(usize::MAX));
    rt.set_max_stack_size(STACK);
    let late = Rc::new(Cell::new(false));
    let flag = late.clone();
    rt.set_interrupt_handler(Some(Box::new(move || {
        let over = Instant::now() >= deadline;
        if over {
            flag.set(true);
        }
        over
    })));
    let Ok(ctx) = Context::full(&rt) else { return result("code mode couldn't start: no context", true) };
    let state = Rc::new(RefCell::new(State::default()));
    let memory = || format!("it used more than {} MB of memory", lim.heap >> 20);
    let stopped =
        |st: &State, why: String| out(&st.logs, st.logged, &format!("The code was stopped: {why}."), true, lim.max);

    // The prelude hands the host `settle` and `run`, kept here: the script can't reach or replace
    // them through the global scope.
    let started: Result<Persistent<Function<'static>>, String> = ctx.with(|ctx| {
        set_host(&ctx, sc, lim, &args, &state).map_err(|e| format!("code mode couldn't start: {e}"))?;
        let start = |_| format!("code mode couldn't start: {}", caught(&ctx));
        let hooks: Object = ctx.eval(PRELUDE).map_err(start)?;
        let (settle, run): (Function, Function) =
            (hooks.get("settle").map_err(start)?, hooks.get("run").map_err(start)?);
        let program = format!("(async () => {{\n{}\n}})", sc.code);
        let script: Function = ctx.eval(program).map_err(|_| format!("The code doesn't parse: {}", caught(&ctx)))?;
        run.call::<_, ()>((script,)).map_err(|_| format!("The code threw: {}", caught(&ctx)))?;
        Ok(Persistent::save(&ctx, settle))
    });
    let settle = match started {
        Ok(settle) => settle,
        Err(why) => {
            let st = state.borrow();
            return if late.get() {
                stopped(&st, format!("it didn't finish in {}", wait_text(lim.wait)))
            } else if why.contains("out of memory") {
                stopped(&st, memory())
            } else {
                out(&st.logs, st.logged, &why, true, lim.max)
            };
        }
    };

    // The event loop: each answer settles its promise, then the jobs it unblocks run, until the
    // script has settled and no call it started is still waiting.
    let mut pending = 0usize;
    loop {
        loop {
            match rt.execute_pending_job() {
                Ok(true) => {}
                Ok(false) => break,
                Err(job) => {
                    let why = job.0.with(|ctx| caught(&ctx));
                    let st = state.borrow();
                    return if late.get() {
                        stopped(&st, format!("it didn't finish in {}", wait_text(lim.wait)))
                    } else if why.contains("out of memory") {
                        stopped(&st, memory())
                    } else {
                        stopped(&st, why)
                    };
                }
            }
        }
        for (id, name, args) in std::mem::take(&mut state.borrow_mut().outbox) {
            pending += 1;
            io.call(id, &name, serde_json::from_str(&args).unwrap_or(json!({})));
        }
        let settled = state.borrow().outcome.is_some();
        if settled && pending == 0 {
            break;
        }
        if pending == 0 {
            let st = state.borrow();
            return out(&st.logs, st.logged, "The code's promise never settled: await each tool call.", true, lim.max);
        }
        let Some(a) = io.answer(deadline) else {
            let st = state.borrow();
            return stopped(&st, format!("it didn't finish in {}", wait_text(lim.wait)));
        };
        pending -= 1;
        let res = a.res.map(|r| r.to_string());
        let settle: Result<(), String> = ctx.with(|ctx| {
            let f = settle.clone().restore(&ctx).map_err(|_| caught(&ctx))?;
            f.call::<_, ()>((a.id as f64, res, a.err)).map_err(|_| caught(&ctx))
        });
        if let Err(why) = settle {
            let st = state.borrow();
            return if late.get() {
                stopped(&st, format!("it didn't finish in {}", wait_text(lim.wait)))
            } else if why.contains("out of memory") {
                stopped(&st, memory())
            } else {
                stopped(&st, why)
            };
        }
    }
    let mut st = state.borrow_mut();
    match st.outcome.take() {
        Some(Outcome::Threw(why)) if why.contains("out of memory") => stopped(&st, memory()),
        Some(Outcome::Threw(why)) => out(&st.logs, st.logged, &format!("The code threw: {why}"), true, lim.max),
        Some(Outcome::Returned(kind, said)) => returned(&st.logs, st.logged, kind, &said, lim.max),
        None => out(&st.logs, st.logged, "The code's promise never settled: await each tool call.", true, lim.max),
    }
}

/// The host's side of the prelude: the four functions it calls and the values it reads, each
/// removed from the global scope once the prelude has taken it.
fn set_host<'js>(
    ctx: &rquickjs::Ctx<'js>,
    sc: &Script,
    lim: &Limits,
    args: &Value,
    state: &Rc<RefCell<State>>,
) -> rquickjs::Result<()> {
    let g = ctx.globals();
    let st = state.clone();
    g.set(
        "__janusCall",
        Function::new(ctx.clone(), move |id: f64, name: String, args: String| {
            st.borrow_mut().outbox.push((id as u64, name, args));
        })?,
    )?;
    let st = state.clone();
    let max = lim.max;
    g.set(
        "__janusLog",
        Function::new(ctx.clone(), move |line: String| {
            let mut st = st.borrow_mut();
            let line = line + "\n";
            if st.logged < max {
                st.logs.push_str(&line);
            }
            st.logged += line.len();
        })?,
    )?;
    let st = state.clone();
    g.set(
        "__janusReturn",
        Function::new(ctx.clone(), move |kind: String, said: String| {
            let kind = match kind.as_str() {
                "none" => "none",
                "string" => "string",
                _ => "shown",
            };
            st.borrow_mut().outcome = Some(Outcome::Returned(kind, said));
        })?,
    )?;
    let st = state.clone();
    g.set(
        "__janusThrow",
        Function::new(ctx.clone(), move |why: String| {
            st.borrow_mut().outcome = Some(Outcome::Threw(why));
        })?,
    )?;
    g.set("__janusLimit", lim.calls as f64)?;
    g.set("__janusToolNames", serde_json::to_string(&sc.tools).unwrap_or_else(|_| "[]".into()))?;
    let srcs: serde_json::Map<String, Value> = sc
        .skills
        .iter()
        .map(|k| {
            let src = skill_source(&k.code).unwrap_or_else(|err| {
                let why = json!(format!("the skill {} can't be run: {err}", k.name));
                format!("(() => {{ throw new Error({why}) }})")
            });
            (k.name.clone(), Value::String(src))
        })
        .collect();
    g.set("__janusSkills", Value::Object(srcs).to_string())?;
    g.set("__janusArgs", args.to_string())?;
    Ok(())
}

/// The exception pending in `ctx`, as text: an Error's name, message and stack.
fn caught(ctx: &rquickjs::Ctx<'_>) -> String {
    let v = ctx.catch();
    if let Some(e) = v.as_exception() {
        let head = e.message().unwrap_or_default();
        let name: Option<String> = e.get("name").ok();
        let head = match name {
            Some(n) if !n.is_empty() => format!("{n}: {head}"),
            _ => head,
        };
        return match e.stack().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
            Some(stack) => format!("{head}\n{stack}"),
            None => head,
        };
    }
    v.as_string().and_then(|s| s.to_string().ok()).unwrap_or_else(|| "an uncaught exception".into())
}

#[cfg(test)]
mod tests {
    use super::super::Skill;
    use super::*;
    use std::collections::VecDeque;
    use std::time::Duration;

    /// Runs each call at once by `tool` and answers them in order; never answers when `tool`
    /// gives `None`.
    struct Fake<F: FnMut(&str, &Value) -> Option<Result<Value, String>>> {
        tool: F,
        answers: VecDeque<Answer>,
        asked: Vec<(String, Value)>,
    }

    impl<F: FnMut(&str, &Value) -> Option<Result<Value, String>>> Io for Fake<F> {
        fn call(&mut self, id: u64, name: &str, args: Value) {
            self.asked.push((name.to_string(), args.clone()));
            match (self.tool)(name, &args) {
                Some(Ok(res)) => self.answers.push_back(Answer { id, res: Some(res), err: None }),
                Some(Err(err)) => self.answers.push_back(Answer { id, res: None, err: Some(err) }),
                None => {}
            }
        }
        fn answer(&mut self, _until: Instant) -> Option<Answer> {
            self.answers.pop_front()
        }
    }

    fn text(s: &str) -> Value {
        json!({ "content": [{ "type": "text", "text": s }] })
    }

    fn lim() -> Limits {
        Limits { wait: Duration::from_secs(5), heap: 64 << 20, ..Limits::DEFAULT }
    }

    /// Runs `code` against tools that echo their name and arguments, with `tools` listed.
    fn go(code: &str, tools: &[&str]) -> (Value, Vec<(String, Value)>) {
        go_with(
            Script {
                code: code.into(),
                args: None,
                tools: tools.iter().map(|s| s.to_string()).collect(),
                skills: vec![],
            },
            lim(),
            |name, args| {
                Some(Ok(
                    json!({ "content": [{ "type": "text", "text": "ok" }], "structuredContent": { "tool": name, "args": args } }),
                ))
            },
        )
    }

    fn go_with(
        sc: Script,
        lim: Limits,
        tool: impl FnMut(&str, &Value) -> Option<Result<Value, String>>,
    ) -> (Value, Vec<(String, Value)>) {
        let mut io = Fake { tool, answers: VecDeque::new(), asked: vec![] };
        let res = run(&sc, &lim, &mut io);
        (res, io.asked)
    }

    fn said(res: &Value) -> &str {
        res["content"][0]["text"].as_str().unwrap_or_default()
    }

    #[test]
    fn calls_tools_one_after_another_and_at_once() {
        let (res, asked) = go(
            r#"const a = await tools.github__search({q: "bug"});
               const [b, c] = await Promise.all([tools["exa__web-search"]({n: 1}), tools.exa__web_search({n: 2})]);
               return [a.tool, b.args.n, c.args.n];"#,
            &["github__search", "exa__web-search"],
        );
        assert_eq!(res["isError"], false, "{res}");
        assert_eq!(said(&res), "[\n  \"github__search\",\n  1,\n  2\n]");
        assert_eq!(asked.len(), 3);
        assert_eq!(asked[2].0, "exa__web-search", "the - alias calls the tool by its own name");
    }

    /// The host's hooks are out of the script's reach: no global names them, and a script that
    /// sets globals by their old names does not keep a tool call from settling.
    #[test]
    fn the_script_cannot_reach_the_hosts_hooks() {
        let names = "Object.getOwnPropertyNames(globalThis).filter(k => k.startsWith('__janus'))";
        let (res, _) = go(&format!("return {names}.length + ':' + typeof __janusSettle;"), &[]);
        assert_eq!(said(&res), "0:undefined", "{res}");
        let code = "globalThis.__janusSettle = () => {}; globalThis.__janusRun = null;
                    return (await tools.t()).tool;";
        let (res, asked) = go(code, &["t"]);
        assert_eq!((said(&res), asked.len()), ("t", 1), "{res}");
    }

    #[test]
    fn a_name_no_tool_listed_is_still_asked_for() {
        let (res, asked) = go("return (await tools.new__tool()).tool;", &[]);
        assert_eq!(said(&res), "new__tool");
        assert_eq!(asked[0].1, json!({}));
    }

    #[test]
    fn a_tool_error_is_thrown_and_can_be_caught() {
        let sc = |code: &str| Script { code: code.into(), args: None, tools: vec!["t".into()], skills: vec![] };
        let failing = |_: &str, _: &Value| {
            Some(Ok(json!({ "content": [{ "type": "text", "text": "rate limited" }], "isError": true })))
        };
        let (res, _) = go_with(sc("await tools.t();"), lim(), failing);
        assert_eq!(res["isError"], true);
        assert!(said(&res).starts_with("The code threw: Error: rate limited"), "{res}");
        let (res, _) =
            go_with(sc("try { await tools.t() } catch (e) { return 'caught ' + e.message }"), lim(), failing);
        assert_eq!(said(&res), "caught rate limited");
        let (res, _) = go_with(sc("await tools.t();"), lim(), |_, _| Some(Err("no tool is called t".into())));
        assert!(said(&res).contains("no tool is called t"), "{res}");
    }

    #[test]
    fn a_result_is_structured_then_json_text_then_text_then_content() {
        let sc = |code: &str| Script { code: code.into(), args: None, tools: vec!["t".into()], skills: vec![] };
        let (res, _) = go_with(sc("const r = await tools.t(); return r.items.length;"), lim(), |_, _| {
            Some(Ok(text(" {\"items\": [1, 2]}")))
        });
        assert_eq!(said(&res), "2");
        let (res, _) = go_with(sc("return typeof (await tools.t());"), lim(), |_, _| Some(Ok(text("plain words"))));
        assert_eq!(said(&res), "string");
        let image = json!({ "content": [{ "type": "image", "data": "AAAA", "mimeType": "image/png" }] });
        let (res, _) = go_with(sc("return await tools.t();"), lim(), move |_, _| Some(Ok(image.clone())));
        assert_eq!(res["content"][0]["type"], "image", "content blocks a script returns go back as content: {res}");
    }

    #[test]
    fn logs_come_before_what_it_returned() {
        let (res, _) = go("console.log('a', {b: 1}); console.error(new Error('x')); return 'done';", &[]);
        let s = said(&res);
        assert!(s.starts_with("a {\"b\":1}\nError: x"), "{s}");
        assert!(s.ends_with("done"), "{s}");
        let (res, _) = go("console.log('only');", &[]);
        assert_eq!(said(&res), "only\n");
        let (res, _) = go("let x = 1;", &[]);
        assert_eq!(said(&res), "The code returned nothing and logged nothing: return what you need.");
    }

    #[test]
    fn a_long_answer_is_cut() {
        let lim = Limits { max: 100, ..lim() };
        let sc = Script { code: "return 'y'.repeat(1000);".into(), args: None, tools: vec![], skills: vec![] };
        let (res, _) = go_with(sc, lim, |_, _| None);
        assert!(said(&res).starts_with(&"y".repeat(100)), "{res}");
        assert!(said(&res).ends_with("… cut: 900 more characters. Return only what you need."), "{res}");
    }

    #[test]
    fn calls_past_the_limit_are_refused() {
        let lim = Limits { calls: 2, ..lim() };
        let sc = Script {
            code: "for (let i = 0; i < 3; i++) await tools.t(); return 'all';".into(),
            args: None,
            tools: vec!["t".into()],
            skills: vec![],
        };
        let (res, asked) = go_with(sc, lim, |_, _| Some(Ok(text("ok"))));
        assert_eq!(asked.len(), 2);
        assert!(said(&res).contains("more than 2 tool calls in one script"), "{res}");
    }

    #[test]
    fn a_promise_never_awaited_to_its_end_is_said() {
        let (res, _) = go("await new Promise(() => {});", &[]);
        assert_eq!(said(&res), "The code's promise never settled: await each tool call.");
    }

    #[test]
    fn bad_code_says_why() {
        let (res, _) = go("return (;", &[]);
        assert!(said(&res).starts_with("The code doesn't parse: SyntaxError"), "{res}");
        let (res, _) = go("throw new TypeError('nope');", &[]);
        assert!(said(&res).starts_with("The code threw: TypeError: nope"), "{res}");
        let (res, _) = go("throw 'a string';", &[]);
        assert_eq!(said(&res), "The code threw: a string");
    }

    #[test]
    fn nothing_of_the_computer_is_there() {
        let (res, _) =
            go("return [typeof require, typeof fetch, typeof setTimeout, typeof std, typeof os].join(',');", &[]);
        assert_eq!(said(&res), "undefined,undefined,undefined,undefined,undefined");
    }

    #[test]
    fn args_are_the_script_inputs() {
        let sc = |args| Script { code: "return args.n * 2;".into(), args, tools: vec![], skills: vec![] };
        let (res, _) = go_with(sc(Some(json!({ "n": 21 }))), lim(), |_, _| None);
        assert_eq!(said(&res), "42");
        let (res, _) = go_with(sc(None), lim(), |_, _| None);
        assert_eq!(said(&res), "null", "no args is an empty object: {res}");
        let (res, _) = go_with(sc(Some(json!([1]))), lim(), |_, _| None);
        assert_eq!(res["isError"], true);
    }

    #[test]
    fn skills_run_their_default_export_with_the_same_tools() {
        let skill = |name: &str, code: &str| Skill { name: name.into(), code: code.into() };
        let sc = Script {
            code: "return [await skills.double_it({n: 2}), await skills['double-it']({n: 3})];".into(),
            args: None,
            tools: vec!["t".into()],
            skills: vec![skill(
                "double-it",
                "const two = 2;\nexport default async (args) => { const r = await tools.t(); return args.n * two + r.length; }",
            )],
        };
        let (res, asked) = go_with(sc, lim(), |_, _| Some(Ok(text("ab"))));
        assert_eq!(said(&res), "[\n  6,\n  8\n]", "{res}");
        assert_eq!(asked.len(), 2);

        let broken = |code: &str| Script {
            code: "return await skills.s();".into(),
            args: None,
            tools: vec![],
            skills: vec![skill("s", code)],
        };
        let (res, _) = go_with(broken("async function f() {}"), lim(), |_, _| None);
        assert!(said(&res).contains("the skill s can't be run: its code.js has no `export default"), "{res}");
        let (res, _) = go_with(broken("import x from 'y';\nexport default async () => 1;"), lim(), |_, _| None);
        assert!(said(&res).contains("imports or exports more than its default"), "{res}");
        let (res, _) = go("return await skills.none();", &[]);
        assert!(said(&res).contains("no skill with code is called none"), "{res}");
    }

    #[test]
    fn a_script_past_its_memory_is_stopped() {
        let lim = Limits { heap: 16 << 20, ..lim() };
        let sc = Script {
            code: "const s = 'x'.repeat(1e8); return s.length;".into(),
            args: None,
            tools: vec![],
            skills: vec![],
        };
        let (res, _) = go_with(sc, lim, |_, _| None);
        assert_eq!(said(&res), "The code was stopped: it used more than 16 MB of memory.", "{res}");
    }

    #[test]
    fn a_script_past_its_time_is_stopped() {
        let lim = Limits { wait: Duration::from_millis(300), ..lim() };
        let sc = |code: &str| Script { code: code.into(), args: None, tools: vec!["t".into()], skills: vec![] };
        let (res, _) = go_with(sc("console.log('started'); while (true) {}"), lim, |_, _| None);
        assert_eq!(said(&res), "started\nThe code was stopped: it didn't finish in 300ms.", "{res}");
        // A tool that never answers: the wait for it is up at the same time.
        let (res, _) = go_with(sc("await tools.t();"), lim, |_, _| None);
        assert_eq!(said(&res), "The code was stopped: it didn't finish in 300ms.", "{res}");
    }
}
