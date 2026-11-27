use super::*;
use crate::lang::Lang::{En, Zh};

fn codex(options: Value) -> Value {
    json!({ "id": "cx", "kind": "codex", "auth": "chatgpt", "options": options })
}

#[test]
fn an_agents_line_says_how_it_signs_in_and_what_it_offers() {
    let kinds = crate::agents::kinds();
    let kind = kinds.as_array().unwrap().iter().find(|k| k["id"] == "codex");
    assert_eq!(connection_summary(&codex(Value::Null), kind, Zh), "Codex，ChatGPT");
    let read = json!({ "model": { "options": [{}, {}], "current": "gpt" } });
    assert_eq!(connection_summary(&codex(read), kind, Zh), "Codex，ChatGPT，2 个模型，未读取权限模式");
    let none = json!({ "controls": [{ "id": "x", "permission": false }] });
    assert_eq!(connection_summary(&codex(none), kind, En), "Codex, ChatGPT, No permission modes");
    let modes = json!({ "controls": [
        { "id": "mode", "permission": true, "name": "Mode", "options": [{ "name": "Ask" }, { "name": "Full" }] },
        { "id": "plan", "permission": true, "type": "boolean", "name": "Plan" },
    ] });
    assert_eq!(connection_summary(&codex(modes), kind, Zh), "Codex，ChatGPT，Ask / Full；Plan");
    let mut api = codex(Value::Null);
    api["auth"] = json!("apikey");
    assert_eq!(connection_summary(&api, kind, En), "Codex, Custom API");
}

#[test]
fn a_check_lists_its_steps_and_while_it_runs_the_ones_to_come() {
    assert!(check_steps(&json!({ "check": [] }), Zh).is_empty(), "nothing before a check");
    let c = json!({ "status": "testing", "check": [
        { "step": "运行组件", "ok": true, "detail": "已安装 1.0" },
        { "step": "凭据", "ok": null, "detail": "…" },
    ] });
    let steps = check_steps(&c, En);
    let states: Vec<_> = steps.iter().map(|s| (s["step"].as_str().unwrap(), s["state"].as_str().unwrap())).collect();
    assert_eq!(
        states,
        [
            ("Runtime", "ok"),
            ("Runtime", "pending"),
            ("Credentials", "running"),
            ("Start Agent", "pending"),
            ("Open Session", "pending"),
            ("Trial Run", "pending")
        ]
    );
    assert_eq!(steps[0]["detail"], "已安装 1.0");
    let done = json!({ "status": "error", "check": [{ "step": "运行时", "ok": false, "detail": "没找到" }] });
    let steps = check_steps(&done, Zh);
    assert_eq!(steps.len(), 1, "a finished check lists only what it recorded");
    assert_eq!((steps[0]["step"].as_str(), steps[0]["state"].as_str()), (Some("运行时"), Some("failed")));
}

#[test]
fn an_environment_reads_as_lines_and_a_template_only_adds() {
    let env = json!({ "B": "two words", "A": "x", "TOKEN": null, "Q": "'quoted'", "H": "a#b" });
    assert_eq!(env_text(&env), "A=x\nB=\"two words\"\nH=\"a#b\"\nQ=\"'quoted'\"\nTOKEN");
    assert_eq!(env_text(&Value::Null), "");
    let template = "export A=template\nexport C=new\n# a comment\n";
    assert_eq!(with_template("A=mine\n", template), "A=mine\nexport C=new");
    assert_eq!(with_template("", template), "export A=template\nexport C=new");
    assert_eq!(with_template("export C\n", template), "export C\nexport A=template", "a bare name counts as set");
    let template =
        "export ANTHROPIC_BASE_URL=https://xxx.xxx.com\nexport ANTHROPIC_AUTH_TOKEN=sk-xxx\nexport ANTHROPIC_MODEL=m";
    assert_eq!(with_template("  \n", template), template);
    let typed = "# my gateway\nANTHROPIC_AUTH_TOKEN\nexport ANTHROPIC_MODEL=mine\n";
    let once = with_template(typed, template);
    assert_eq!(
        once,
        "# my gateway\nANTHROPIC_AUTH_TOKEN\nexport ANTHROPIC_MODEL=mine\nexport ANTHROPIC_BASE_URL=https://xxx.xxx.com"
    );
    assert_eq!(with_template(&once, template), once, "twice adds nothing more");
    assert_eq!(env_name("KEY = value"), Some("KEY"));
}

#[test]
fn a_permission_choice_gets_an_icon_by_what_it_says() {
    let icon = |v: &str, n: &str| permission_icon(&json!({ "value": v, "name": n }));
    assert_eq!(icon("read-only", "Read Only"), ("eye", false));
    assert_eq!(icon("danger-full-access", "Full access"), ("exclamationmark.shield", true));
    assert_eq!(icon("bypassPermissions", "Bypass"), ("exclamationmark.shield", true));
    assert_eq!(icon("acceptEdits", "Accept edits"), ("pencil", false));
    assert_eq!(icon("default", "Ask"), ("checkmark.shield", false));
}

#[test]
fn a_sign_in_log_loses_its_colors_and_gives_its_links() {
    let (clean, urls) = login_log("\u{1b}[1;32mOpen\u{1b}[0m https://auth.example.com/x?a=1 then <https://b.example>");
    assert_eq!(clean, "Open https://auth.example.com/x?a=1 then <https://b.example>");
    assert_eq!(urls, ["https://auth.example.com/x?a=1", "https://b.example>"]);
}

#[test]
fn a_dressed_agent_carries_its_lines_and_permission_icons() {
    let mut c = json!({
        "id": "cx", "kind": "codex", "auth": "chatgpt", "env": { "K": "v" }, "loginLog": "\u{1b}[0mgo https://x.io",
        "options": { "controls": [{ "id": "mode", "permission": true, "name": "Mode", "options": [{ "value": "read-only", "name": "Read" }] }] },
    });
    dress_connection(&mut c, &crate::agents::kinds(), Zh);
    assert_eq!(c["envText"], "K=v");
    assert_eq!(c["loginLabel"], "ChatGPT");
    assert_eq!(c["loginLog"], "go https://x.io");
    assert_eq!(c["loginUrls"], json!(["https://x.io"]));
    assert_eq!(c["options"]["controls"][0]["options"][0]["icon"], "eye");
    let mut api = json!({ "id": "c2", "kind": "claude", "auth": "apikey" });
    dress_connection(&mut api, &crate::agents::kinds(), Zh);
    assert!(api["loginLabel"].is_null(), "no account sign-in");
}

#[test]
fn a_digital_employees_lines_say_what_it_does_and_runs_on() {
    let conns = vec![json!({
        "id": "cx",
        "options": {
            "model": { "current": "gpt-5" },
            "controls": [
                { "id": "mode", "permission": true, "default": "auto", "options": [{ "value": "auto", "name": "Auto" }, { "value": "ro", "name": "Read" }] },
                { "id": "plan", "permission": true, "type": "boolean", "default": false, "name": "Plan" },
            ],
        },
    })];
    let mut r = json!({ "id": "dev", "name": "dev ops", "init": "", "connection": "cx", "session": { "plan": true }, "prompt": "do" });
    dress_role(&mut r, &conns, Zh);
    assert_eq!(r["displayInit"], "DE");
    assert_eq!(r["line"], "还没有填写职责 · Auto · Plan 开启 · 提示词");
    assert_eq!(r["modelLine"], "gpt-5");
    let mut r = json!({ "id": "x", "name": "评审", "init": "评", "desc": "看代码", "connection": "cx", "model": "o3", "effort": "high", "session": { "mode": "ro" } });
    dress_role(&mut r, &conns, En);
    assert_eq!(r["displayInit"], "评");
    assert_eq!(r["line"], "看代码 · Read · Plan Off");
    assert_eq!(r["modelLine"], "o3, high");
    let mut unbound = json!({ "id": "y", "name": "y", "connection": "" });
    dress_role(&mut unbound, &conns, Zh);
    assert!(unbound["modelLine"].is_null());
    assert_eq!(unbound["line"], "还没有填写职责");
}

#[test]
fn a_stat_line_counts_what_went_well_and_what_did_not() {
    assert_eq!(stat_line(&json!({ "tasks": 0 }), 3, Zh), None);
    let x = json!({ "tasks": 10, "firstPass": 7, "failed": 2, "verificationFailed": 0, "avgMinutes": 5.2 });
    assert_eq!(
        stat_line(&x, 27, Zh).unwrap(),
        "最近 27 个任务：10 个子任务 · 首次通过 70% · 失败 2 · 成功平均 5.2 分钟"
    );
    let y = json!({ "tasks": 3, "firstPass": 1, "failed": 0, "verificationFailed": 1, "avgMinutes": 0.0 });
    assert_eq!(
        stat_line(&y, 5, En).unwrap(),
        "Last 5 runs: Subtasks: 3 · Passed first time: 33% · Verification failures: 1"
    );
}
