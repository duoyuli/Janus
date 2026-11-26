//! `run_code`, the router's code mode, after magpie's (`hub_code.go`): a script that calls the
//! tools behind the router and gives back only what it returns and logs. It runs in a process of
//! its own (`crate::coderun`); each tool call it makes goes as `call_tool`'s does, recorded as
//! `mcp.routed`. The member's skills with a `code.js` are functions on `skills`; a script that ran
//! without an error can be saved as one (`save_as`), a skill of the library's for every digital
//! employee from the next run. The person turns code mode off in the library (`App::code_mode`).
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

use super::*;
use crate::coderun::{Limits, Script, Skill};

/// The tool of `run_code`.
pub(super) const CODE: &str = "run_code";
/// How many skills with code `run_code`'s description names, and how much of each one's
/// description.
const SKILLS_SAID: usize = 30;
const SKILL_ABOUT: usize = 200;

/// A skill with code the session can call: its name, what it does, its `code.js`.
pub(super) struct CodeSkill {
    name: String,
    about: String,
    code: String,
}

/// `run_code` as `tools/list` gives it: what it does, how a script reaches the tools, the limits,
/// the skills with code it can call, and how to keep a script.
pub(super) fn spec(skills: &[CodeSkill], lang: Lang) -> Value {
    let l = Limits::DEFAULT;
    let (calls, secs) = (l.calls, l.wait.as_secs());
    let mut about = tr!(
        lang,
        "运行一段调用这个服务器背后工具的 JavaScript，只交回代码 return 和 console.log 的内容。几次工具调用一步完成（先后、循环，或用 Promise.all 同时），或只留下工具结果里需要的部分时用它。每个工具是 `tools` 上的一个 async 函数，按名字调用：`const r = await tools[\"github__search_issues\"]({{q: \"bug\"}})`（也可以写 tools.github__search_issues，- 写成 _）；工具的名字和参数用 find_tools 查。工具交回它的结构化结果，否则是文本（是 JSON 时解析好）；工具出错时抛出。代码是一个 async 函数的函数体：用 return 交回结果。只有纯 JavaScript：没有 require、import、fetch、文件和定时器。一段代码最多调用 {calls} 次工具，总共最多 {secs} 秒。",
        "Runs JavaScript that calls the tools behind this server, and gives back only what the code returns and console.logs. Use it for several tool calls in one step (one after another, in a loop, or at once with Promise.all), or to keep only the part of a tool's answer you need. Each tool is an async function on `tools`, by its name: `const r = await tools[\"github__search_issues\"]({{q: \"bug\"}})` (also tools.github__search_issues, with - as _); find_tools gives each tool's name and parameters. A tool gives back its structured result, else its text (parsed when it is JSON); a tool's error is thrown. The code is the body of an async function: `return` the result. Plain JavaScript only: no require, import, fetch, files or timers. At most {calls} tool calls and {secs}s in all."
    );
    if !skills.is_empty() {
        about += &tr!(
            lang,
            "资源库里带代码的 Skill 是 `skills` 上的 async 函数，各接一个 args 对象，调用同样的工具：",
            " The library's skills with code are async functions on `skills`, each given one object of args, calling the same tools:"
        );
        let named: Vec<String> = skills
            .iter()
            .take(SKILLS_SAID)
            .map(|k| {
                let about = crate::text::one_line(&k.about, SKILL_ABOUT);
                let key = k.name.replace('-', "_");
                if about.is_empty() {
                    format!("skills.{key}")
                } else {
                    format!("skills.{key}{}{about}{}", lang.pick("（", " ("), lang.pick("）", ")"))
                }
            })
            .collect();
        about += &named.join(lang.pick("；", "; "));
        if skills.len() > SKILLS_SAID {
            let more = skills.len() - SKILLS_SAID;
            about += &tr!(lang, "；另有 {more} 个", "; and {more} more");
        }
        about += lang.pick("。", ".");
    }
    about += &tr!(
        lang,
        "想留下跑通的代码下次再用，带上 save_as 运行：它会存成资源库的一个 Skill，之后每个数字员工都能用 skills.<名字>(args) 调用，代码从 `args` 读输入。",
        " To keep code that worked for next time, run it with save_as: it is saved as a skill of the library's, skills.<name>(args) for every digital employee from then on, reading its inputs from `args`."
    );
    json!({
        "name": CODE,
        "description": about,
        "inputSchema": { "type": "object", "properties": {
            "code": { "type": "string", "description": lang.pick("JavaScript，一个 async 函数的函数体；return 你要的结果", "JavaScript, the body of an async function; return what you need") },
            "args": { "type": "object", "description": lang.pick("代码以 args 对象读到的输入", "Inputs the code reads as the object args") },
            "save_as": {
                "type": "object",
                "description": lang.pick("代码没有出错地跑完时，再把它存成资源库的一个 Skill，之后用 skills.<名字>(args) 调用", "Also save the code, when it runs without an error, as a skill of the library's for skills.<name>(args) later"),
                "properties": {
                    "name": { "type": "string", "description": lang.pick("小写英文单词，用 - 连接", "lowercase words joined by -") },
                    "description": { "type": "string", "description": lang.pick("它做什么、什么时候用、接收哪些 args", "what it does and when to use it, and the args it takes") },
                },
                "required": ["name", "description"],
            },
        }, "required": ["code"] },
    })
}

/// The program a script runs in: Janus itself, started again as `coderun::COMMAND`. Unit tests
/// run as a test binary, so they start the `janus` built beside it.
fn helper() -> Result<PathBuf> {
    #[cfg(test)]
    return Ok(crate::coderun::test_exe());
    #[cfg(not(test))]
    std::env::current_exe().context("找不到 Janus 自己的程序")
}

impl App {
    /// The skills with code the session `seat` can call: those in its member's snapshot with a
    /// `code.js`, by name.
    pub(super) fn code_skills(&self, run: &Run, seat: &SessionKey) -> Vec<CodeSkill> {
        let mut out: Vec<CodeSkill> = session_member(run, seat)
            .into_iter()
            .flat_map(|m| m.resources.iter().filter(|i| i.kind() == ItemKind::Skill))
            .filter_map(|i| {
                let code = self.skill_code(i)?;
                Some(CodeSkill { name: i.name.clone(), about: i.description.clone(), code })
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Answer `run_code`: the script run against the session's tools, each call as `call_tool`
    /// makes it, until the script ends or the agent cancels the request (`stop`); then, with
    /// `save_as`, the script kept as a skill when it ran without an error.
    pub(super) async fn run_code(
        self: &Arc<Self>,
        run: &Arc<Run>,
        seat: &SessionKey,
        args: &Value,
        stop: &CancellationToken,
    ) -> Value {
        let lang = run.lang;
        let code = args["code"].as_str().unwrap_or_default();
        if code.trim().is_empty() {
            return tool_text(
                &tr!(
                    lang,
                    "run_code 需要 code：一个 async 函数的函数体，调用 tools[\"<服务器>__<工具>\"](args) 并 return 你要的结果。",
                    "run_code needs code: the body of an async function that calls tools[\"<server>__<tool>\"](args) and returns what you need."
                ),
                true,
            );
        }
        let exe = match helper() {
            Ok(exe) => exe,
            Err(err) => return tool_text(&format!("{err:#}"), true),
        };
        let tools = self.routed_tools(run, seat, tokio::time::Instant::now() + LIST_WAIT, false).await;
        let script = Script {
            code: code.to_string(),
            args: args.get("args").filter(|a| !a.is_null()).cloned(),
            tools: tools.iter().map(|t| t.name.clone()).collect(),
            skills: self.code_skills(run, seat).into_iter().map(|k| Skill { name: k.name, code: k.code }).collect(),
        };
        let (app, r, s, token) = (self.clone(), run.clone(), seat.clone(), stop.clone());
        let tools = Arc::new(tools);
        let call = move |name: String, inner: Value| {
            let (app, run, seat, stop, tools) = (app.clone(), r.clone(), s.clone(), token.clone(), tools.clone());
            async move {
                let tool = match find::named(&tools, &name) {
                    Some(t) => t.clone(),
                    // A tool added since the script was given its names is still found.
                    None => match app.named_tool(&run, &seat, &name).await {
                        (Some(t), _) => t,
                        (None, now) => bail!(find::no_such(&now, &name, run.lang)),
                    },
                };
                Ok(app.call_routed(&run, &seat, &tool, inner, &stop).await)
            }
        };
        let res = tokio::select! {
            res = crate::coderun::run(&exe, script, Limits::DEFAULT, call) => res,
            () = stop.cancelled() => return tool_text(&tr!(lang, "调用已取消", "The call was cancelled"), true),
        };
        match args["save_as"].as_object() {
            Some(save) => self.save_script(res, save, code, lang).await,
            None => res,
        }
    }

    /// `res`, what a script gave back, with what became of its `save_as` in a last text block.
    async fn save_script(
        &self,
        mut res: Value,
        save: &serde_json::Map<String, Value>,
        code: &str,
        lang: Lang,
    ) -> Value {
        let text = |k: &str| save.get(k).and_then(Value::as_str).unwrap_or_default().trim().to_string();
        let name = text("name");
        let say = if res["isError"] == true {
            tr!(lang, "没有存成 Skill：代码没有跑通。", "Not saved as a skill: the code didn't run without an error.")
        } else {
            let file = format!(
                "// {}\nexport default async (args) => {{\n{}\n}}\n",
                lang.pick(
                    "从 Janus 的 run_code 存下。它在那里以 skills.<名字>(args) 运行，以 tools.<服务器>__<工具>(args) 调用 MCP 路由的工具。",
                    "Saved from Janus's run_code. It runs there as skills.<name>(args), calling the MCP router's tools as tools.<server>__<tool>(args).",
                ),
                code.trim_end()
            );
            match self.save_code_skill(&name, &text("description"), &file).await {
                Ok(item) => {
                    let key = item.name.replace('-', "_");
                    tr!(
                        lang,
                        "已存成 Skill {}：从下一个任务起，skills.{key}(args) 运行它。",
                        "Saved as the skill {}: skills.{key}(args) runs it from the next run on.",
                        item.name
                    )
                }
                Err(err) => tr!(lang, "没有存成 Skill：{err:#}", "Not saved as a skill: {err:#}"),
            }
        };
        if let Some(content) = res["content"].as_array_mut() {
            content.push(json!({ "type": "text", "text": say }));
        }
        res
    }
}
