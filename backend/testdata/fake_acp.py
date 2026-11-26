#!/usr/bin/env python3
"""A scripted ACP agent for orchestrator tests.

FAKE_DIR holds script.json, which maps a task id to the outcome of each successive prompt for it:
"ok", "fail", "cancelled", "slow", "write:<file>", "edit:<file>" (reports an edit of the file with ACP
`locations`, then stays busy for two seconds so tasks overlap), "unmet" (reports its first acceptance criterion
unmet), "methods" (reports every criterion with the next evidence method in test, command, inspection,
reasoning, unverified, and three pieces of task-level evidence), "badmethod" (reports the first criterion as met with
`method: "vibes"`, which the result check must refuse), "permission" (asks session/request_permission
with three options), "elicit" (asks elicitation/create with a form), "loop" (makes the same tool call five times
before its result), "progress" (edits one file and runs tests in turn, then edits it five more times, every
call with a different diff or output), "ask" (ends the turn with a question; the answer comes back as the next prompt), "hold:<file>"
(stays at work until FAKE_DIR/<file> exists, then approves) or "rich" (thinks,
publishes a plan and its slash commands, runs a tool that stays in progress for a moment and then reports a file diff,
reports its context usage and switches its own mode; the prompt response carries token usage). The last outcome repeats.
Usage outcomes (for the read-only ledger): "usage" reports three context snapshots and a cost, and its prompt response
carries 1200 input / 300 output / 1500 total for the main loop plus `_meta.quota.model_usage` rows for the main model
(1500) and a subagent (500), the shape the Claude Code and Codex adapters send; "usage-quiet" reports a context snapshot
but no token usage at all; "usage-turns" asks a question in the first turn (1000 tokens) and reports 2500 for the
answered turn: per-turn numbers that grow, which add up to 3500.
Records outcomes write the session's transcript as Claude Code does, into
`$CLAUDE_CONFIG_DIR/projects/<cwd>/<sid>.jsonl`: one response streamed on two lines (its last one counts: 300 input,
80 output, 100 cache reads, 480 in all) and a subagent's response in `<sid>/subagents/` (80 in all). "records" then
hands in and returns a prompt response claiming 1500 tokens, which must not count again; "records-crash" exits
mid-turn without answering, as an agent that dies.
session/set_mode is logged ("mode <id>"), and the resource links of each prompt ("links <name>…").
With a `goal-sticky` file the lead reports its /goal active and never clears it; Janus's ask whether
it is done is answered "GOAL_DONE" with the lead-go text, and `_session/goal` clear is logged ("goal clear").
As a goal-mode lead it logs whether its brief arrived in the system layer (`_meta.systemPrompt.append`,
as Janus gives Claude Code: "brief ok", "brief ok resumed"), then works on "/goal …" until FAKE_DIR/lead-go
exists and ends with the text of that file. A lead whose session was loaded after a restart gets Janus's resume
prompt instead: it logs "lead resumed <sid> <prompt as JSON>", works until lead-go, then reports its goal cleared.
With a `lead-exit` file the lead's process exits once on its "/goal" prompt (the file is removed), as an agent that dies mid-goal;
with a `lead-dies` file it exits on every "/goal" and every Janus resume prompt.
With a `mute` file in FAKE_DIR it never answers `initialize`, like an agent that hangs at startup.
With a `steer` file in FAKE_DIR it declares `_session/steering`: a steer that arrives during
a prompt is logged ("steer <text>") and answered "injected".
A prompt that is not a task's and names "[write <file>]" appends to that file in the session's directory, reports the
edit as a tool call and ends the turn (a lead asked for more work after delivery); with "[commit]" in it too, the agent commits everything there first.
A chat with one digital employee (a session in a `/chats/` directory) logs "chat session <cwd>" and answers every
prompt "回复：<prompt>" ("[slow]" in it keeps the turn going until it is cancelled; "[stream]" writes the reply in 40
lines, one chunk every 0.15 s, as an agent writing a long answer does; "[team]" first proposes the
employees in FAKE_DIR/team.json with janus.propose_employees, as 招聘顾问 hands over the team it designed, and logs
"propose taken|refused").
A workspace Loop's session (its system layer opens "You are the Loop of the workspace", or 「你是工作区…的值班」 in Chinese) logs "loop wake <n>" for each wake
and answers "看过了：<first line>"; on its first wake it titles the session ("标题：<the prompt's last line>",
as `session_info_update`, which must not name the Loop); "[write <file>]" in the Loop's prompt appends to that file in the workspace first,
and "[hold]" keeps the turn going until FAKE_DIR/loop-go exists (then removes it). Each wake's prompt is logged
("loop prompt <json>"); with a `loop-exit` file in FAKE_DIR the agent removes it and exits in the middle of the wake.
The person's own messages to a Loop are answered "回复：<prompt>".
A consolidation of a workspace's team memory (its system layer says "consolidating this workspace's team memory", or
「整理这个工作区的团队记忆」 in Chinese)
logs "dream prompt <json>" and keeps its one turn going until FAKE_DIR/dream-go exists, then ends it with that file's
text (and removes it), so a test can call the memory tools meanwhile.
With a `content.json` in FAKE_DIR the agent says and writes what the file gives it (e2e/screenshots.py stages
demonstration data this way): the outcome "content" works on its task as `tasks.<task id>` describes (`thought`, `plan`,
`files` it writes and reports as edits with diffs, `commands` of which the last stays in progress while it works,
`context` and `cost` for a usage snapshot, `permission` to ask for first, `hold` naming a file in FAKE_DIR to wait for,
`seconds` at work, then a structured result from `summary`, `handoff`, `checks` (evidence, method and passed for each
criterion in turn), `evidence`, `risks` and `verdict`, with `tokens` as the turn's usage); a lead whose goal starts with a
key of `goals` works until FAKE_DIR/go-<key> exists and ends with that goal's `report`, `tokens` and `cost`; a Loop titles
its session `loop_title` and answers a wake with `loops.<first line of its prompt>`, else `loop_default`.
A session's task and criteria are kept in FAKE_DIR (`session-<id>.json`), so a session loaded after a restart
continues its task. Session setup (session/new `mcpServers`, set_config_option) and the image blocks of each
prompt are appended to calls.log; it declares HTTP MCP support and image input. Counters live in FAKE_DIR so the
script carries across the separate processes of separate attempts.
"""

import fcntl
import json
import os
import re
import select
import sys

# @project-doc docs/operations/20261031-testing.md#fake_agent
DIR = os.environ["FAKE_DIR"]


def content():
    """FAKE_DIR/content.json: what to say and write instead of the scripted strings; empty without the file."""
    try:
        with open(os.path.join(DIR, "content.json")) as f:
            return json.load(f)
    except FileNotFoundError:
        return {}


def send(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def say(sid, text):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": sid, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}})


def snapshot(sid, used, size, cost=None):
    """A context snapshot (`usage_update`); `cost` is the agent's own cumulative session cost."""
    update = {"sessionUpdate": "usage_update", "used": used, "size": size}
    if cost:
        update["cost"] = cost
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": update}})


def tokens(total):
    """A prompt response's token usage, split so both sides can be checked."""
    return {"inputTokens": total * 4 // 5, "outputTokens": total - total * 4 // 5, "totalTokens": total}


CRITERIA = {}  # session id -> acceptance criteria from the task prompt


METHODS = ["test", "command", "inspection", "reasoning", "unverified"]
METHOD_EVIDENCE = {
    "test": "运行 cargo test，全部通过",
    "command": "运行 just status，输出正常",
    "inspection": "读了 backend/src/evidence.rs 的解析逻辑",
    "reasoning": "根据调用方推断不会影响其他路径",
    "unverified": "本机没有 Xcode，暂时无法核实界面",
}


def write_records(sid):
    """The session's transcript in the agent's config directory, as Claude Code keeps it."""
    project = os.path.join(os.environ["CLAUDE_CONFIG_DIR"], "projects", re.sub(r"[^A-Za-z0-9]", "-", os.getcwd()))
    os.makedirs(os.path.join(project, sid, "subagents"), exist_ok=True)

    def line(mid, model, output, **usage):
        return json.dumps({"type": "assistant", "requestId": "req-" + mid, "message": {"id": mid, "model": model, "usage": {
            "input_tokens": usage.get("input", 0), "output_tokens": output,
            "cache_read_input_tokens": usage.get("read", 0), "cache_creation_input_tokens": 0}}}) + "\n"
    with open(os.path.join(project, sid + ".jsonl"), "a") as f:
        f.write(json.dumps({"type": "user", "message": {"role": "user", "content": "go"}}) + "\n")
        f.write(line("m1", "claude-main", 50, input=300, read=100))
        f.write(line("m1", "claude-main", 80, input=300, read=100))
    with open(os.path.join(project, sid, "subagents", "agent-1.jsonl"), "a") as f:
        f.write(line("m2", "claude-sub", 20, input=60))


def result(sid=None, unmet=False, methods=None, **fields):
    """A result to hand in; every acceptance criterion of the session's task is checked (the first
    one reported unmet when `unmet`). `methods` gives each check the next evidence method. A
    `question` field makes it a question for the lead instead (`hand_in`)."""
    checks = []
    for i, c in enumerate(CRITERIA.get(sid, [])):
        failed = unmet and i == 0
        method = methods[i % len(methods)] if methods else None
        check = {"criterion": c, "passed": not failed,
                 "evidence": "没做到" if failed else METHOD_EVIDENCE.get(method, "fake check")}
        if method:
            check["method"] = method
        checks.append(check)
    body = {"summary": "完成", "checks": checks, "evidence": ["fake evidence"], "risks": []}
    body.update(fields)
    return body


def tool(sid, name, args):
    """Call one of the janus server's tools from this session, as an agent does: (text, is_error)."""
    import urllib.request
    server = JANUS.get(sid)
    if not server:
        return "no janus server", True
    headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
    headers.update({h["name"]: h["value"] for h in server.get("headers", [])})
    payload = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}
    req = urllib.request.Request(server["url"], data=json.dumps(payload).encode(), headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=60) as r:
        out = json.loads(r.read() or b"{}")
    res = out.get("result") or {}
    return "".join(c.get("text", "") for c in res.get("content", [])), bool(res.get("isError"))


def hand_in(sid, body):
    """Hand in what the turn came to the way an agent does: a question with janus.ask, otherwise the
    result with janus.submit (logged "submit <task> taken|refused"); the reply says it in a line.
    Told it may fix an unmet criterion and hand in once more, it does when its script's next step
    for the task is "ok"."""
    body = dict(body)
    question = body.pop("question", None)
    task = TASKS.get(sid, "unknown")
    if question:
        tool(sid, "ask", question)
        log(f"ask {task}")
        say(sid, body.get("summary") or question.get("text", ""))
        return
    text, err = tool(sid, "submit", body)
    log(f"submit {task} {'refused' if err else 'taken'}")
    if not err and ("再交一次" in text or "hand in once more" in text) and upcoming(task) == "ok":
        next_outcome(task)
        fixed = result(sid, summary=f"{task} 修好后完成")
        text, err = tool(sid, "submit", fixed)
        log(f"submit {task} {'refused' if err else 'taken'} again")
    say(sid, body.get("summary", ""))


def upcoming(task):
    """The next scripted outcome for `task`, without taking it."""
    steps = json.load(open(os.path.join(DIR, "script.json"))).get(task, ["ok"])
    counter = os.path.join(DIR, f"count-{task}")
    n = int(open(counter).read()) if os.path.exists(counter) else 0
    return steps[min(n, len(steps) - 1)]


def next_outcome(task):
    script = json.load(open(os.path.join(DIR, "script.json")))
    steps = script.get(task, ["ok"])
    counter = os.path.join(DIR, f"count-{task}")
    # Agents of one FAKE_DIR run as separate processes (two connection checks at once share
    # `count-unknown`): read and bump the count under a lock, and replace the file whole, so no
    # process reads it half written.
    with open(counter + ".lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        n = int(open(counter).read()) if os.path.exists(counter) else 0
        with open(counter + ".tmp", "w") as f:
            f.write(str(n + 1))
        os.replace(counter + ".tmp", counter)
    with open(os.path.join(DIR, "calls.log"), "a") as log:
        log.write(f"{task} {steps[min(n, len(steps) - 1)]}\n")
    return steps[min(n, len(steps) - 1)]


def remember(sid, task, criteria):
    """A session's task and criteria, kept in FAKE_DIR: a session loaded by a later process
    (session/load after a restart) continues the same task without being told it again."""
    with open(os.path.join(DIR, f"session-{sid}.json"), "w") as f:
        json.dump({"task": task, "criteria": criteria}, f, ensure_ascii=False)


def remembered(sid):
    try:
        with open(os.path.join(DIR, f"session-{sid}.json")) as f:
            return json.load(f)
    except FileNotFoundError:
        return {}


def log(line):
    with open(os.path.join(DIR, "calls.log"), "a") as f:
        f.write(line + "\n")


# A `plan-on` file in FAKE_DIR starts sessions in Plan mode, as an agent whose own default is Plan would.
CONFIG = {"mode": "ask", "plan_mode": os.path.exists(os.path.join(DIR, "plan-on"))}
TASKS = {}  # session id -> task id, for follow-up prompts that do not repeat the task


def config_options():
    return [
        {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": CONFIG["mode"],
         "options": [{"value": v, "name": v.title()} for v in ("read-only", "ask", "agent", "full")]},
        {"id": "plan_mode", "name": "Plan", "type": "boolean", "currentValue": CONFIG["plan_mode"]},
    ]


def ask_client(method, params):
    """Send a request to the client and block until its response arrives."""
    rid = f"fake-{method}"
    send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
    for line in sys.stdin:
        msg = json.loads(line)
        if msg.get("id") == rid and "method" not in msg:
            return msg.get("result")
    sys.exit(0)


def wait_cancel(seconds):
    """Sleep, but stop early when the client sends session/cancel. Steering joins meanwhile."""
    import time
    end = time.time() + seconds
    while time.time() < end:
        ready, _, _ = select.select([sys.stdin], [], [], 0.1)
        if ready:
            line = sys.stdin.readline()
            if not line:
                sys.exit(0)
            msg = json.loads(line)
            if msg.get("method") == "session/cancel":
                return True
            if msg.get("method") == "session/set_config_option" and "id" in msg:
                # A control switched while the turn runs, as the person can from the session.
                CONFIG[msg["params"]["configId"]] = msg["params"]["value"]
                log(f"set {msg['params']['configId']} {json.dumps(msg['params']['value'])}")
                send({"jsonrpc": "2.0", "id": msg["id"], "result": {"configOptions": config_options()}})
            if msg.get("method") == "_session/steering" and "id" in msg:
                p = msg["params"]
                log("steer " + " ".join(b.get("text", "") for b in p["prompt"]).replace("\n", " "))
                links = [b["name"] for b in p["prompt"] if b.get("type") == "resource_link"]
                if links:
                    log("links " + " ".join(links))
                send({"jsonrpc": "2.0", "id": msg["id"], "result": {"outcome": "injected"}})
    return False


def content_turn(sid, task, d):
    """One turn on `task` as its entry `d` of content.json describes (see the module comment)."""
    def upd(u):
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": u}})

    plan = d.get("plan") or []

    def show_plan(done):
        if plan:
            upd({"sessionUpdate": "plan", "entries": [
                {"content": p, "priority": "high", "status": "completed" if i < done else "in_progress" if i == done else "pending"}
                for i, p in enumerate(plan)]})

    if d.get("thought"):
        upd({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": d["thought"]}})
    show_plan(0)
    if wait_cancel(d.get("seconds", 0) / 2):
        return {"stopReason": "cancelled"}
    for i, (name, body) in enumerate((d.get("files") or {}).items()):
        path = os.path.join(os.getcwd(), name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        old = open(path).read() if os.path.exists(path) else ""
        with open(path, "w") as f:
            f.write(body)
        upd({"sessionUpdate": "tool_call", "toolCallId": f"edit-{task}-{i}", "title": f"Edit {name}", "kind": "edit",
             "status": "completed", "locations": [{"path": path}], "rawInput": {"file_path": name},
             "content": [{"type": "diff", "path": path, "oldText": old, "newText": body}]})
    show_plan(max(0, len(plan) - 1))
    if d.get("context"):
        snapshot(sid, d["context"], 200000, {"amount": d["cost"], "currency": "USD"} if d.get("cost") else None)
    commands = d.get("commands") or []
    for i, c in enumerate(commands):
        last = i == len(commands) - 1
        upd({"sessionUpdate": "tool_call", "toolCallId": f"run-{task}-{i}", "title": c["title"], "kind": "execute",
             "status": "in_progress" if last else "completed", "rawInput": {"command": c["title"].split()},
             **({} if last else {"rawOutput": c.get("output", "")})})
    if d.get("permission"):
        ask_client("session/request_permission", {"sessionId": sid, "toolCall": {
            "toolCallId": f"perm-{task}", "title": d["permission"], "rawInput": {"command": d["permission"].split()}},
            "options": [{"optionId": "a1", "name": "Allow once", "kind": "allow_once"},
                        {"optionId": "a2", "name": "Always allow", "kind": "allow_always"},
                        {"optionId": "r1", "name": "Reject", "kind": "reject_once"}]})
    if d.get("hold"):
        gate = os.path.join(DIR, d["hold"])
        while not os.path.exists(gate):
            if wait_cancel(0.2):
                return {"stopReason": "cancelled"}
    if wait_cancel(d.get("seconds", 0) / 2):
        return {"stopReason": "cancelled"}
    if commands:
        upd({"sessionUpdate": "tool_call_update", "toolCallId": f"run-{task}-{len(commands) - 1}", "status": "completed",
             "rawOutput": commands[-1].get("output", "")})
    show_plan(len(plan))
    given = d.get("checks") or []
    checks = []
    for i, criterion in enumerate(CRITERIA.get(sid, [])):
        g = given[i] if i < len(given) else {}
        checks.append({"criterion": criterion, "passed": g.get("passed", True), "method": g.get("method", "test"),
                       "evidence": g.get("evidence", "")})
    body = {"summary": d.get("summary", ""), "checks": checks, "evidence": d.get("evidence", []),
            "risks": d.get("risks", [])}
    for k in ("handoff", "verdict"):
        if d.get(k):
            body[k] = d[k]
    hand_in(sid, body)
    return {"stopReason": "end_turn", "usage": tokens(d.get("tokens", 1500))}


def prompt(msg):
    sid = msg["params"]["sessionId"]
    text = msg["params"]["prompt"][0]["text"]
    if re.search(r"^You are the Loop of the workspace|^你是工作区「[^」]*」（[^）]*）的值班", SYSTEM.get(sid, "")):
        # A wake sends the Loop's prompt as it is; the tests talk to a Loop as the person with "[me] ".
        if text.startswith("[me] "):
            say(sid, "回复：" + text[len("[me] "):])
            return {"stopReason": "end_turn"}
        log("loop prompt " + json.dumps(text, ensure_ascii=False))
        broken = os.path.join(DIR, "loop-exit")
        if os.path.exists(broken):
            os.remove(broken)
            os._exit(1)
        if sid not in TITLED:
            TITLED.add(sid)
            send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
                "sessionUpdate": "session_info_update", "title": content().get("loop_title") or "标题：" + text.strip().splitlines()[-1][:20]}}})
        w = re.search(r"\[write (\S+)\]", text)
        if w:
            with open(w.group(1), "a") as f:
                f.write("loop\n")
        if "[hold]" in text:
            go = os.path.join(DIR, "loop-go")
            while not os.path.exists(go):
                if wait_cancel(0.2):
                    return {"stopReason": "cancelled"}
            os.remove(go)
        given = content()
        say(sid, (given.get("loops") or {}).get(text.strip().splitlines()[0]) or given.get("loop_default") or "看过了：" + text.splitlines()[0])
        return {"stopReason": "end_turn"}
    if re.search(r"consolidating this workspace's team memory|整理这个工作区的团队记忆", SYSTEM.get(sid, "")):
        log("dream prompt " + json.dumps(text, ensure_ascii=False))
        go = os.path.join(DIR, "dream-go")
        while not os.path.exists(go):
            if wait_cancel(0.2):
                return {"stopReason": "cancelled"}
        say(sid, open(go).read())
        os.remove(go)
        return {"stopReason": "end_turn"}
    if sid in CHATS:
        if "[slow]" in text and wait_cancel(600):
            return {"stopReason": "cancelled"}
        if "[stream]" in text:
            for i in range(40):
                say(sid, f"第 {i + 1} 行：一边写一边出现在会话里，视图跟着往下走。\n\n")
                if wait_cancel(0.15):
                    return {"stopReason": "cancelled"}
        if "[team]" in text and os.path.isfile(os.path.join(DIR, "team.json")):
            # As 招聘顾问 does: the team it designed, handed to the person to create.
            with open(os.path.join(DIR, "team.json")) as f:
                _, err = tool(sid, "propose_employees", {"employees": json.load(f)})
            log(f"propose {'refused' if err else 'taken'}")
        say(sid, "回复：" + text)
        return {"stopReason": "end_turn"}
    if text.startswith(("[Janus] 你的这一轮已经结束", "[Janus] Your turn has ended")):
        # Janus asks whether a goal that never settled is done.
        log("goal check")
        say(sid, "GOAL_DONE\n" + open(os.path.join(DIR, "lead-go")).read())
        return {"stopReason": "end_turn"}
    if os.path.exists(os.path.join(DIR, "lead-dies")) and (text.startswith("/goal ") or text.startswith("[Janus] ")):
        # A lead whose agent dies whenever it starts on its goal.
        log("lead dies")
        os._exit(1)
    if text.startswith(("[Janus] Janus restarted while you were working on this goal.",
                        "[Janus] Your session broke off while you were working on this goal",
                        "[Janus] 你推进这个目标时 Janus 重启了", "[Janus] 你推进这个目标时会话中断了")):
        # A lead whose own session was loaded after a restart: told what happened, it carries on
        # with its goal (kept with the session) until lead-go, then reports it met.
        log("lead resumed " + sid + " " + json.dumps(text, ensure_ascii=False))
        go = os.path.join(DIR, "lead-go")
        while not os.path.exists(go):
            if wait_cancel(0.2):
                return {"stopReason": "cancelled"}
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "session_info_update", "_meta": {"jetbrains": {"air": {"goal": None}}}}}})
        say(sid, open(go).read())
        return {"stopReason": "end_turn"}
    if text.startswith("/goal "):
        log("goal " + text[len("/goal "):])
        broken = os.path.join(DIR, "lead-exit")
        if os.path.exists(broken):
            # The lead's agent dies once in the middle of its goal (its session stays on record).
            remember(sid, "lead", [])
            os.remove(broken)
            os._exit(1)
        sticky = os.path.exists(os.path.join(DIR, "goal-sticky"))
        if sticky:
            # Like Claude Code whose own check never runs: the goal is reported active and stays so.
            send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
                "sessionUpdate": "session_info_update", "_meta": {"jetbrains": {"air": {"goal": {"objective": text[6:], "status": "active"}}}}}}})
        import time
        # A goal `content.json` knows ends on its own file with its own report, so several leads share one FAKE_DIR.
        goal = text[len("/goal "):].strip()
        given = next((v for k, v in (content().get("goals") or {}).items() if goal.startswith(k)), None)
        go = os.path.join(DIR, "go-" + given["key"] if given else "lead-go")
        while not os.path.exists(go):
            if wait_cancel(0.2):
                return {"stopReason": "cancelled"}
        if not given:
            say(sid, open(go).read())
            return {"stopReason": "end_turn"}
        if given.get("cost"):
            snapshot(sid, given.get("context", 30000), 200000, {"amount": given["cost"], "currency": "USD"})
        say(sid, given["report"])
        return {"stopReason": "end_turn", "usage": tokens(given.get("tokens", 1500))}
    w = re.search(r"\[write (\S+)\]", text)
    if w and not subtask_of(text):
        # A follow-up asking for a file: written in the session's directory, the turn ends.
        with open(w.group(1), "a") as f:
            f.write("follow-up\n")
        # Reported like any edit, so Janus knows of it without Git too.
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "tool_call", "toolCallId": f"follow-up-{w.group(1)}", "title": f"Edit {w.group(1)}", "kind": "edit",
            "status": "completed", "locations": [{"path": os.path.join(os.getcwd(), w.group(1))}], "rawInput": {"file_path": w.group(1)}}}})
        if "[commit]" in text:
            # Asked to commit: the agent commits in its own checkout (a worktree's detached HEAD).
            import subprocess
            subprocess.run(["git", "add", "-A"], check=True)
            subprocess.run(["git", "-c", "user.name=fake", "-c", "user.email=fake@fake", "commit", "-q", "-m", "follow-up"], check=True)
        say(sid, f"已写 {w.group(1)}")
        return {"stopReason": "end_turn"}
    m = subtask_of(text)
    known = remembered(sid)
    task = m or TASKS.get(sid, known.get("task", "unknown"))
    TASKS[sid] = task
    c = re.search(r"(?:验收标准：|Acceptance criteria:)\n((?:- .*\n?)+)", text) if m else None
    if c:
        CRITERIA[sid] = [line[2:].strip() for line in c.group(1).splitlines() if line.startswith("- ")]
    elif sid not in CRITERIA and known.get("criteria"):
        CRITERIA[sid] = known["criteria"]
    if m:
        remember(sid, task, CRITERIA.get(sid, []))
    outcome = next_outcome(task)
    if outcome in ("usage", "usage-quiet", "usage-turns"):
        if outcome == "usage-quiet":
            # Context only: this session never reports tokens, so its usage stays 「未上报」.
            snapshot(sid, 18000, 200000)
            hand_in(sid, result(sid))
            return {"stopReason": "end_turn"}
        if outcome == "usage":
            # The same snapshot twice, then a bigger one: only the latest and the peak are shown.
            snapshot(sid, 52000, 200000)
            snapshot(sid, 52000, 200000)
            snapshot(sid, 61000, 200000, {"amount": 0.25, "currency": "USD"})
            hand_in(sid, result(sid))
            # `usage` is the main loop only; the quota rows by model also count a subagent.
            quota = {"model_usage": [
                {"model": "main", "token_count": {"totalTokens": 1500, "inputTokens": 1000, "cachedInputTokens": 200,
                                                  "cachedWriteTokens": 0, "outputTokens": 300, "reasoningOutputTokens": 0}},
                {"model": "subagent", "token_count": {"totalTokens": 500, "inputTokens": 400, "cachedInputTokens": 0,
                                                      "cachedWriteTokens": 0, "outputTokens": 100, "reasoningOutputTokens": 0}},
            ]}
            return {"stopReason": "end_turn", "usage": tokens(1500), "_meta": {"quota": quota}}
        # Two turns in one session: the first asks a question, the answered turn reports its own, larger number.
        if answered(text) is not None:
            hand_in(sid, result(sid, summary=f"{task} 按回答完成"))
            return {"stopReason": "end_turn", "usage": tokens(2500)}
        hand_in(sid, result(sid, summary="需要确认", question={"text": "用哪个名字？", "options": ["alpha", "beta"]}))
        return {"stopReason": "end_turn", "usage": tokens(1000)}
    if outcome in ("records", "records-crash"):
        write_records(sid)
        if outcome == "records-crash":
            os._exit(1)
        hand_in(sid, result(sid))
        return {"stopReason": "end_turn", "usage": tokens(1500)}
    if outcome == "content":
        return content_turn(sid, task, content()["tasks"][task])
    if outcome == "fail":
        raise RuntimeError("scripted failure")
    if outcome == "cancelled":
        return {"stopReason": "cancelled"}
    if outcome == "slow":
        # 并行跑全套时场景可能超过一分钟，slow 只应在显式取消时结束。
        if wait_cancel(600):
            return {"stopReason": "cancelled"}
    if outcome.startswith("edit:"):
        name = outcome.split(":", 1)[1]
        with open(name, "a") as f:
            f.write(f"{task}\n")
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "tool_call", "toolCallId": f"edit-{task}", "title": f"Edit {name}", "kind": "edit",
            "status": "completed", "locations": [{"path": os.path.join(os.getcwd(), name)}], "rawInput": {"file_path": name}}}})
        if wait_cancel(2):
            return {"stopReason": "cancelled"}
    if outcome.startswith("skill:"):
        # Like an agent using a library skill: it loads janus-skills from the bundle session/new
        # handed it (`BUNDLE`); the catalog there says where each skill's SKILL.md is; read both,
        # as Claude Code's Read does.
        name = outcome.split(":", 1)[1]
        routers = [os.path.join(b, d, "janus-skills", "SKILL.md") for b in BUNDLE.get(sid, []) for d in ("skills", ".agents/skills")]
        router = next((r for r in routers if os.path.isfile(r)), None)
        found = False
        if router:
            catalog = open(router).read()
            base = re.search(r"`(\S+)/<(?:名字|name)>/SKILL\.md`", catalog)
            path = os.path.join(base.group(1), name, "SKILL.md") if base else ""
            found = bool(re.search(rf"^- {re.escape(name)}[：:]", catalog, re.M)) and os.path.isfile(path)
            for i, p in enumerate([router] + ([path] if found else [])):
                send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
                    "sessionUpdate": "tool_call", "toolCallId": f"skill-{task}-{i}", "title": f"Read {os.path.basename(p)}",
                    "kind": "read", "status": "completed", "locations": [{"path": p}], "rawInput": {"file_path": p}}}})
        log(f"skill {name} {'read' if found else 'missing'}")
    if outcome.startswith("write:"):
        name = outcome.split(":", 1)[1]
        with open(name, "a") as f:
            f.write(f"{task}\n")
    if outcome == "permission":
        r = ask_client("session/request_permission", {"sessionId": sid, "toolCall": {"toolCallId": "t1", "title": "rm -rf build", "rawInput": {"command": ["rm", "-rf", "build"]}},
                       "options": [{"optionId": "a1", "name": "Allow once", "kind": "allow_once"},
                                   {"optionId": "a2", "name": "Always allow", "kind": "allow_always"},
                                   {"optionId": "r1", "name": "Reject", "kind": "reject_once"}]})
        hand_in(sid, result(sid, summary=f"picked {r['outcome'].get('optionId')}"))
        return {"stopReason": "end_turn"}
    if outcome == "elicit":
        r = ask_client("elicitation/create", {"sessionId": sid, "mode": "form", "message": "Which name?",
                       "requestedSchema": {"type": "object", "properties": {"name": {"type": "string", "title": "Name"}}, "required": ["name"]}})
        hand_in(sid, result(sid, summary=f"{r['action']} {r.get('content', {}).get('name')}"))
        return {"stopReason": "end_turn"}
    if outcome == "loop":
        for i in range(5):
            send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
                "sessionUpdate": "tool_call", "toolCallId": f"loop-{i}", "title": "cat missing.txt", "kind": "read",
                "status": "completed", "rawInput": {"command": ["cat", "missing.txt"]}}}})
    if outcome == "loop2":
        for i in range(8):
            even = i % 2 == 0
            send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
                "sessionUpdate": "tool_call", "toolCallId": f"loop2-{i}",
                "title": "Edit a.rs" if even else "cargo test",
                "kind": "edit" if even else "execute",
                "status": "completed",
                "rawInput": {"file_path": "a.rs"} if even else {"command": ["cargo", "test"]}}}})
    if outcome == "progress":
        # Edit/test in turn with a changing diff and output, then five edits of the same file with
        # different text: steady work that loop detection must not report.
        path = os.path.join(os.getcwd(), "a.rs")
        steps = [i % 2 == 0 for i in range(10)] + [True] * 5
        for i, editing in enumerate(steps):
            update = {"sessionUpdate": "tool_call", "toolCallId": f"progress-{i}", "status": "completed"}
            if editing:
                update.update(title="Edit a.rs", kind="edit", rawInput={"file_path": "a.rs", "new_string": f"v{i}"},
                              content=[{"type": "diff", "path": path, "oldText": f"v{i - 1}", "newText": f"v{i}"}])
            else:
                update.update(title="cargo test", kind="execute", rawInput={"command": ["cargo", "test"]},
                              rawOutput=f"{10 - i} failed")
            send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": update}})
    if outcome == "busy-tool":
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "tool_call", "toolCallId": f"busy-{task}", "title": "cargo test --workspace",
            "kind": "execute", "status": "in_progress", "rawInput": {"command": ["cargo", "test", "--workspace"]}}}})
        if wait_cancel(2.5):
            return {"stopReason": "cancelled"}
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "tool_call_update", "toolCallId": f"busy-{task}", "status": "completed"}}})
    if outcome == "high-ctx-fail":
        # A session nearly full that then breaks off: a retry does not go back into it.
        snapshot(sid, 160000, 200000)
        # Let the snapshot be read before the failed reply ends the turn.
        import time
        time.sleep(0.3)
        raise RuntimeError("scripted failure")
    if outcome == "high-ctx":
        snapshot(sid, 160000, 200000)
        hand_in(sid, result(sid, summary=f"{task} 高上下文完成"))
        return {"stopReason": "end_turn", "usage": tokens(1500)}
    if outcome == "handoff":
        with open("api.rs", "w") as f:
            f.write("pub fn parse() {}\n")
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "tool_call", "toolCallId": f"edit-{task}", "title": "Edit api.rs", "kind": "edit",
            "status": "completed", "locations": [{"path": os.path.join(os.getcwd(), "api.rs")}],
            "rawInput": {"file_path": "api.rs"}}}})
        hand_in(sid, result(sid, methods=["reasoning"], summary=f"{task} 完成核心接口",
                        handoff="接口签名：parse(input: &str) -> Result<Ast>，错误统一用 ParseError",
                        risks=["空字符串边界尚未压测"]))
        return {"stopReason": "end_turn", "usage": tokens(1200)}
    if outcome == "reject":
        checks = [{"criterion": c, "passed": False, "method": "test", "evidence": "运行 cargo test 复现空输入崩溃"}
                  for c in CRITERIA.get(sid, [])]
        body = {"summary": f"{task} 完成独立检查", "verdict": "rejected", "checks": checks,
                "evidence": ["cargo test parse_empty panic"], "risks": ["空输入直接崩溃"]}
        hand_in(sid, body)
        return {"stopReason": "end_turn", "usage": tokens(800)}
    if outcome.startswith("hold:"):
        # Stays at work until FAKE_DIR/<name> exists, then approves (a verification still running
        # while its target is reworked).
        gate = os.path.join(DIR, outcome.split(":", 1)[1])
        while not os.path.exists(gate):
            if wait_cancel(0.2):
                return {"stopReason": "cancelled"}
        outcome = "approve"
    if outcome == "approve":
        checks = [{"criterion": c, "passed": True, "method": "test", "evidence": "运行 cargo test 全部通过"}
                  for c in CRITERIA.get(sid, [])]
        body = {"summary": f"{task} 独立验证通过", "verdict": "approved", "checks": checks,
                "evidence": ["cargo test 0 failed"], "risks": []}
        hand_in(sid, body)
        return {"stopReason": "end_turn", "usage": tokens(600)}
    if outcome == "rich":
        def upd(u):
            send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": u}})
        upd({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "先看看 pay.js "}})
        upd({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "再改。"}})
        upd({"sessionUpdate": "plan", "entries": [{"content": "读代码", "status": "completed", "priority": "high"},
                                                   {"content": "改 pay.js", "status": "in_progress", "priority": "high"}]})
        upd({"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "compact", "description": "压缩上下文"},
                                                                                  {"name": "review", "description": "审查改动", "input": {"hint": "要审查什么"}}]})
        upd({"sessionUpdate": "tool_call", "toolCallId": "rich-1", "title": "Edit pay.js", "kind": "edit", "status": "in_progress",
             "rawInput": {"file_path": "pay.js"}})
        import time
        time.sleep(1.5)
        with open("pay.js", "a") as f:
            f.write("sdk()\n")
        upd({"sessionUpdate": "tool_call_update", "toolCallId": "rich-1", "status": "completed",
             "locations": [{"path": os.path.join(os.getcwd(), "pay.js")}],
             "content": [{"type": "diff", "path": os.path.join(os.getcwd(), "pay.js"), "oldText": "a\n", "newText": "a\nsdk()\n"}]})
        upd({"sessionUpdate": "usage_update", "used": 52000, "size": 200000})
        upd({"sessionUpdate": "current_mode_update", "currentModeId": "agent"})
        hand_in(sid, result(sid))
        return {"stopReason": "end_turn", "usage": {"inputTokens": 1200, "outputTokens": 300, "totalTokens": 1500}}
    if outcome == "unmet":
        hand_in(sid, result(sid, unmet=True, summary=f"{task} 没有全部做到"))
        return {"stopReason": "end_turn"}
    if outcome == "methods":
        hand_in(sid, result(sid, methods=METHODS, summary=f"{task} 用不同方法核对",
                      evidence=["运行 cargo test 通过", "运行 just status 正常", "手工核对界面输出"]))
        return {"stopReason": "end_turn"}
    if outcome == "badmethod":
        checks = [{"criterion": c, "passed": True, "evidence": "看着没问题", "method": "vibes"}
                  for c in CRITERIA.get(sid, [])]
        body = {"summary": f"{task} 按感觉完成", "checks": checks, "evidence": ["看着没问题"], "risks": []}
        hand_in(sid, body)
        return {"stopReason": "end_turn"}
    if outcome == "ask":
        hand_in(sid, result(sid, summary="需要确认", question={"text": "用哪个名字？", "options": ["alpha", "beta"], "recommend": "alpha"}))
        return {"stopReason": "end_turn"}
    if outcome == "ok" and answered(text) is not None:
        answer = answered(text)
        hand_in(sid, result(sid, summary=f"{task} 按回答 {answer} 完成"))
        return {"stopReason": "end_turn"}
    hand_in(sid, result(sid, summary=f"{task} 完成"))
    return {"stopReason": "end_turn"}


SYSTEM = {}  # session id -> what Janus put in its system layer (`system_layer`)
JANUS = {}  # session id -> the janus MCP server session/new or session/load gave it (url, headers)
BUNDLE = {}  # session id -> the skill bundles session/new or session/load handed it (`bundles`)
TITLED = set()  # sessions that titled themselves (a Loop's first wake)
CHATS = set()  # session ids of chats with one digital employee: their prompts are answered "回复：<text>"


def subtask_of(text):
    """The subtask id a task prompt gives, in either language ("你的子任务 T1：" / "Your subtask T1:")."""
    m = re.search(r"你的子任务 (\S+)：|Your subtask (\S+):", text)
    return (m.group(1) or m.group(2)) if m else None


def answered(text):
    """The answer in a prompt telling the agent its question was answered, in either language; None otherwise."""
    m = re.search(r"你的问题已得到回答：(.*)|Your question has been answered: (.*)", text)
    return (m.group(1) if m.group(1) is not None else m.group(2)) if m else None


def remember_janus(sid, params):
    """The janus server among the session's `mcpServers`, for handing in through its tools."""
    for server in params.get("mcpServers") or []:
        if server.get("name") == "janus":
            JANUS[sid] = server


def system_layer(params):
    """Claude Code's `_meta.systemPrompt.append`, else Codex's `developer_instructions` from the environment."""
    return ((params.get("_meta") or {}).get("systemPrompt") or {}).get("append") or json.loads(
        os.environ.get("CODEX_CONFIG") or "{}").get("developer_instructions", "")


def bundles(params):
    """The bundles of skills Janus hands a session in `_meta`: Claude Code's local plugins, Codex's additional roots."""
    meta = params.get("_meta") or {}
    plugins = ((meta.get("claudeCode") or {}).get("options") or {}).get("plugins") or []
    return [p["path"] for p in plugins if p.get("path")] + list(meta.get("additionalRoots") or [])


def log_layer(method, sid, params):
    """The system layer a session was opened or loaded with, on one line ("layer <new|load> <sid> <json>")."""
    log(f"layer {method} {sid} {json.dumps(system_layer(params))}")


def log_brief(params):
    """A lead's brief arrives in the system layer: Claude Code's `_meta.systemPrompt.append`. A chat's
    session is logged with its working directory ("chat session <cwd>")."""
    system = ((params.get("_meta") or {}).get("systemPrompt") or {}).get("append", "")
    if "你是 Janus 团队的负责人" in system or "You are the lead of the Janus team" in system:
        log("brief ok" + (" resumed" if "从中断处继续：" in system or "Resuming after an interruption:" in system else ""))
    if "/chats/" in params.get("cwd", ""):
        # A chat: Janus adds no instructions of its own; the role's, if any, follow.
        log("chat session " + params.get("cwd", "") + (" janus-brief" if "There is no team" in system else ""))
        return True
    return False


for line in sys.stdin:
    msg = json.loads(line)
    method, mid = msg.get("method"), msg.get("id")
    if mid is None:
        continue
    try:
        if method == "initialize":
            if os.path.exists(os.path.join(DIR, "mute")):
                import time
                time.sleep(3600)
            caps = {"mcpCapabilities": {"http": True}, "promptCapabilities": {"image": True}}
            if not os.path.exists(os.path.join(DIR, "no-load")):
                caps["loadSession"] = True
            res = {"protocolVersion": 1, "agentInfo": {"name": "fake"}, "agentCapabilities": caps}
            if os.path.exists(os.path.join(DIR, "steer")):
                res["_meta"] = {"steering": {"supported": True}}
        elif method == "session/new":
            chat = log_brief(msg["params"])
            log("mcp " + json.dumps(msg["params"].get("mcpServers", []), sort_keys=True))
            res = {"sessionId": "s-" + str(os.getpid()), "configOptions": config_options()}
            SYSTEM[res["sessionId"]] = system_layer(msg["params"])
            BUNDLE[res["sessionId"]] = bundles(msg["params"])
            remember_janus(res["sessionId"], msg["params"])
            log_layer("new", res["sessionId"], msg["params"])
            if chat:
                CHATS.add(res["sessionId"])
        elif method == "session/load":
            if os.path.exists(os.path.join(DIR, "fail-load")):
                raise RuntimeError("session load failed")
            sid = msg["params"]["sessionId"]
            log(f"load {sid}")
            SYSTEM[sid] = system_layer(msg["params"])
            BUNDLE[sid] = bundles(msg["params"])
            remember_janus(sid, msg["params"])
            log_layer("load", sid, msg["params"])
            if log_brief(msg["params"]):
                CHATS.add(sid)
            log("mcp " + json.dumps(msg["params"].get("mcpServers", []), sort_keys=True))
            res = {"sessionId": sid, "configOptions": config_options()}
        elif method == "session/set_config_option":
            CONFIG[msg["params"]["configId"]] = msg["params"]["value"]
            log(f"set {msg['params']['configId']} {json.dumps(msg['params']['value'])}")
            res = {"configOptions": config_options()}
        elif method == "_session/goal":
            log(f"goal {msg['params']['action']}")
            send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": msg["params"]["sessionId"], "update": {
                "sessionUpdate": "session_info_update", "_meta": {"jetbrains": {"air": {"goal": None}}}}}})
            res = {}
        elif method == "session/set_mode":
            log(f"mode {msg['params']['modeId']}")
            res = {}
        elif method == "session/prompt":
            images = [b for b in msg["params"]["prompt"] if b.get("type") == "image"]
            if images:
                log("images " + " ".join(f'{b["mimeType"]}:{len(b["data"])}' for b in images))
            links = [b for b in msg["params"]["prompt"] if b.get("type") == "resource_link"]
            if links:
                log("links " + " ".join(b["name"] for b in links))
            res = prompt(msg)
        else:
            res = {}
        send({"jsonrpc": "2.0", "id": mid, "result": res})
    except Exception as err:  # noqa: BLE001 - scripted failures are reported as JSON-RPC errors
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32000, "message": str(err)}})
