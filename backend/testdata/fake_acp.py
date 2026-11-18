#!/usr/bin/env python3
"""A scripted ACP agent for orchestrator tests.

FAKE_DIR holds script.json, which maps a task id to the outcome of each successive prompt for it:
"ok", "fail", "cancelled", "slow", "write:<file>", "edit:<file>" (reports an edit of the file with ACP
`locations`, then stays busy for two seconds so tasks overlap), "unmet" (reports its first acceptance criterion
unmet), "methods" (reports every criterion with the next evidence method in test, command, inspection,
reasoning, unverified, and three pieces of task-level evidence), "badmethod" (reports the first criterion as met with
`method: "vibes"`, which the result check must refuse), "permission" (asks session/request_permission
with three options), "elicit" (asks elicitation/create with a form), "loop" (makes the same tool call five times
before its result), "ask" (ends the turn with a question; the answer comes back as the next prompt) or "rich" (thinks,
publishes a plan and its slash commands, runs a tool that stays in progress for a moment and then reports a file diff,
reports its context usage and switches its own mode; the prompt response carries token usage). The last outcome repeats.
Usage outcomes (for the read-only ledger): "usage" reports three context snapshots and a cost, and its prompt response
carries 1200 input / 300 output / 1500 total for the main loop plus `_meta.quota.model_usage` rows for the main model
(1500) and a subagent (500), the shape the Claude Code and Codex adapters send; "usage-quiet" reports a context snapshot
but no token usage at all; "usage-turns" asks a question in the first turn (1000 tokens) and reports 2500 for the
answered turn: per-turn numbers that grow, which add up to 3500.
session/set_mode is logged ("mode <id>"), and the resource links of each prompt ("links <name>…").
With a `goal-sticky` file the lead reports its /goal active and never clears it; Janus's ask whether
it is done is answered "GOAL_DONE" with the lead-go text, and `_session/goal` clear is logged ("goal clear").
As a goal-mode lead it acknowledges the brief, then works on "/goal …" until FAKE_DIR/lead-go
exists and ends with the text of that file.
With a `steer` file in FAKE_DIR it declares `_session/steering`: a steer that arrives during
a prompt is logged ("steer <text>") and answered "injected".
Session setup (session/new `mcpServers`, set_config_option) and the image blocks of each
prompt are appended to calls.log; it declares HTTP MCP support and image input. Counters live in FAKE_DIR so the
script carries across the separate processes of separate attempts.
"""

import json
import os
import re
import select
import sys

# @project-doc docs/operations/20261031-testing.md#fake_agent
DIR = os.environ["FAKE_DIR"]


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


def result(sid=None, unmet=False, methods=None, **fields):
    """A structured result; every acceptance criterion of the session's task is checked (the first
    one reported unmet when `unmet`). `methods` gives each check the next evidence method."""
    checks = []
    for i, c in enumerate(CRITERIA.get(sid, [])):
        failed = unmet and i == 0
        method = methods[i % len(methods)] if methods else None
        check = {"criterion": c, "passed": not failed,
                 "evidence": "没做到" if failed else METHOD_EVIDENCE.get(method, "fake check")}
        if method:
            check["method"] = method
        checks.append(check)
    body = {"summary": "完成", "checks": checks, "evidence": ["fake evidence"], "risks": [], "question": None}
    body.update(fields)
    return "```json\n" + json.dumps(body, ensure_ascii=False) + "\n```"


def next_outcome(task):
    script = json.load(open(os.path.join(DIR, "script.json")))
    steps = script.get(task, ["ok"])
    counter = os.path.join(DIR, f"count-{task}")
    n = int(open(counter).read()) if os.path.exists(counter) else 0
    open(counter, "w").write(str(n + 1))
    with open(os.path.join(DIR, "calls.log"), "a") as log:
        log.write(f"{task} {steps[min(n, len(steps) - 1)]}\n")
    return steps[min(n, len(steps) - 1)]


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
            if msg.get("method") == "_session/steering" and "id" in msg:
                p = msg["params"]
                log("steer " + " ".join(b.get("text", "") for b in p["prompt"]).replace("\n", " "))
                send({"jsonrpc": "2.0", "id": msg["id"], "result": {"outcome": "injected"}})
    return False


def prompt(msg):
    sid = msg["params"]["sessionId"]
    text = msg["params"]["prompt"][0]["text"]
    if "Wait for the goal before starting work." in text:
        log("brief " + ("janus.orchestrate" in text and "ok" or "missing") + (" resumed" if "Resuming after an interruption." in text else ""))
        say(sid, "Acknowledged")
        return {"stopReason": "end_turn"}
    if text.startswith("[Janus] 你的这一轮已经结束"):
        # Janus asks whether a goal that never settled is done.
        log("goal check")
        say(sid, "GOAL_DONE\n" + open(os.path.join(DIR, "lead-go")).read())
        return {"stopReason": "end_turn"}
    if text.startswith("/goal "):
        log("goal " + text[len("/goal "):])
        sticky = os.path.exists(os.path.join(DIR, "goal-sticky"))
        if sticky:
            # Like Claude Code whose own check never runs: the goal is reported active and stays so.
            send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
                "sessionUpdate": "session_info_update", "_meta": {"jetbrains": {"air": {"goal": {"objective": text[6:], "status": "active"}}}}}}})
        import time
        go = os.path.join(DIR, "lead-go")
        while not os.path.exists(go):
            if wait_cancel(0.2):
                return {"stopReason": "cancelled"}
        say(sid, open(go).read())
        return {"stopReason": "end_turn"}
    m = re.search(r"你的子任务 (\S+)：", text)
    task = m.group(1) if m else TASKS.get(sid, "unknown")
    TASKS[sid] = task
    c = re.search(r"验收标准：\n((?:- .*\n?)+)", text) if m else None
    if c:
        CRITERIA[sid] = [line[2:].strip() for line in c.group(1).splitlines() if line.startswith("- ")]
    outcome = next_outcome(task)
    if outcome in ("usage", "usage-quiet", "usage-turns"):
        if outcome == "usage-quiet":
            # Context only: this session never reports tokens, so its usage stays 「未上报」.
            snapshot(sid, 18000, 200000)
            say(sid, result(sid))
            return {"stopReason": "end_turn"}
        if outcome == "usage":
            # The same snapshot twice, then a bigger one: only the latest and the peak are shown.
            snapshot(sid, 52000, 200000)
            snapshot(sid, 52000, 200000)
            snapshot(sid, 61000, 200000, {"amount": 0.25, "currency": "USD"})
            say(sid, result(sid))
            # `usage` is the main loop only; the quota rows by model also count a subagent.
            quota = {"model_usage": [
                {"model": "main", "token_count": {"totalTokens": 1500, "inputTokens": 1000, "cachedInputTokens": 200,
                                                  "cachedWriteTokens": 0, "outputTokens": 300, "reasoningOutputTokens": 0}},
                {"model": "subagent", "token_count": {"totalTokens": 500, "inputTokens": 400, "cachedInputTokens": 0,
                                                      "cachedWriteTokens": 0, "outputTokens": 100, "reasoningOutputTokens": 0}},
            ]}
            return {"stopReason": "end_turn", "usage": tokens(1500), "_meta": {"quota": quota}}
        # Two turns in one session: the first asks a question, the answered turn reports its own, larger number.
        if "你的问题已得到回答" in text:
            say(sid, result(sid, summary=f"{task} 按回答完成"))
            return {"stopReason": "end_turn", "usage": tokens(2500)}
        say(sid, result(sid, summary="需要确认", question={"text": "用哪个名字？", "options": ["alpha", "beta"]}))
        return {"stopReason": "end_turn", "usage": tokens(1000)}
    if outcome == "fail":
        raise RuntimeError("scripted failure")
    if outcome == "cancelled":
        return {"stopReason": "cancelled"}
    if outcome == "slow":
        if wait_cancel(60):
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
    if outcome.startswith("write:"):
        name = outcome.split(":", 1)[1]
        with open(name, "a") as f:
            f.write(f"{task}\n")
    if outcome == "permission":
        r = ask_client("session/request_permission", {"sessionId": sid, "toolCall": {"toolCallId": "t1", "title": "rm -rf build", "rawInput": {"command": ["rm", "-rf", "build"]}},
                       "options": [{"optionId": "a1", "name": "Allow once", "kind": "allow_once"},
                                   {"optionId": "a2", "name": "Always allow", "kind": "allow_always"},
                                   {"optionId": "r1", "name": "Reject", "kind": "reject_once"}]})
        say(sid, result(sid, summary=f"picked {r['outcome'].get('optionId')}"))
        return {"stopReason": "end_turn"}
    if outcome == "elicit":
        r = ask_client("elicitation/create", {"sessionId": sid, "mode": "form", "message": "Which name?",
                       "requestedSchema": {"type": "object", "properties": {"name": {"type": "string", "title": "Name"}}, "required": ["name"]}})
        say(sid, result(sid, summary=f"{r['action']} {r.get('content', {}).get('name')}"))
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
    if outcome == "busy-tool":
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "tool_call", "toolCallId": f"busy-{task}", "title": "cargo test --workspace",
            "kind": "execute", "status": "in_progress", "rawInput": {"command": ["cargo", "test", "--workspace"]}}}})
        if wait_cancel(2.5):
            return {"stopReason": "cancelled"}
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "tool_call_update", "toolCallId": f"busy-{task}", "status": "completed"}}})
    if outcome == "cancel-usage":
        if wait_cancel(60):
            return {"stopReason": "cancelled", "usage": tokens(900)}
    if outcome == "high-ctx":
        snapshot(sid, 160000, 200000)
        say(sid, result(sid, summary=f"{task} 高上下文完成"))
        return {"stopReason": "end_turn", "usage": tokens(1500)}
    if outcome == "handoff":
        with open("api.rs", "w") as f:
            f.write("pub fn parse() {}\n")
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "tool_call", "toolCallId": f"edit-{task}", "title": "Edit api.rs", "kind": "edit",
            "status": "completed", "locations": [{"path": os.path.join(os.getcwd(), "api.rs")}],
            "rawInput": {"file_path": "api.rs"}}}})
        say(sid, result(sid, methods=["reasoning"], summary=f"{task} 完成核心接口",
                        handoff="接口签名：parse(input: &str) -> Result<Ast>，错误统一用 ParseError",
                        risks=["空字符串边界尚未压测"]))
        return {"stopReason": "end_turn", "usage": tokens(1200)}
    if outcome == "reject":
        checks = [{"criterion": c, "passed": False, "method": "test", "evidence": "运行 cargo test 复现空输入崩溃"}
                  for c in CRITERIA.get(sid, [])]
        body = {"summary": f"{task} 完成独立检查", "verdict": "rejected", "checks": checks,
                "evidence": ["cargo test parse_empty panic"], "risks": ["空输入直接崩溃"], "question": None}
        say(sid, "```json\n" + json.dumps(body, ensure_ascii=False) + "\n```")
        return {"stopReason": "end_turn", "usage": tokens(800)}
    if outcome == "approve":
        checks = [{"criterion": c, "passed": True, "method": "test", "evidence": "运行 cargo test 全部通过"}
                  for c in CRITERIA.get(sid, [])]
        body = {"summary": f"{task} 独立验证通过", "verdict": "approved", "checks": checks,
                "evidence": ["cargo test 0 failed"], "risks": [], "question": None}
        say(sid, "```json\n" + json.dumps(body, ensure_ascii=False) + "\n```")
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
        say(sid, result(sid))
        return {"stopReason": "end_turn", "usage": {"inputTokens": 1200, "outputTokens": 300, "totalTokens": 1500}}
    if outcome == "unmet":
        say(sid, result(sid, unmet=True, summary=f"{task} 没有全部做到"))
        return {"stopReason": "end_turn"}
    if outcome == "methods":
        say(sid, result(sid, methods=METHODS, summary=f"{task} 用不同方法核对",
                      evidence=["运行 cargo test 通过", "运行 just status 正常", "手工核对界面输出"]))
        return {"stopReason": "end_turn"}
    if outcome == "badmethod":
        checks = [{"criterion": c, "passed": True, "evidence": "看着没问题", "method": "vibes"}
                  for c in CRITERIA.get(sid, [])]
        body = {"summary": f"{task} 按感觉完成", "checks": checks, "evidence": ["看着没问题"], "risks": [], "question": None}
        say(sid, "```json\n" + json.dumps(body, ensure_ascii=False) + "\n```")
        return {"stopReason": "end_turn"}
    if outcome == "ask":
        say(sid, result(sid, summary="需要确认", question={"text": "用哪个名字？", "options": ["alpha", "beta"]}))
        return {"stopReason": "end_turn"}
    if outcome == "ok" and "你的问题已得到回答" in text:
        answer = text.split("你的问题已得到回答：", 1)[1].split("\n", 1)[0]
        say(sid, result(sid, summary=f"{task} 按回答 {answer} 完成"))
        return {"stopReason": "end_turn"}
    say(sid, result(sid, summary=f"{task} 完成"))
    return {"stopReason": "end_turn"}


for line in sys.stdin:
    msg = json.loads(line)
    method, mid = msg.get("method"), msg.get("id")
    if mid is None:
        continue
    try:
        if method == "initialize":
            caps = {"mcpCapabilities": {"http": True}, "promptCapabilities": {"image": True}}
            if not os.path.exists(os.path.join(DIR, "no-load")):
                caps["loadSession"] = True
            res = {"protocolVersion": 1, "agentInfo": {"name": "fake"}, "agentCapabilities": caps}
            if os.path.exists(os.path.join(DIR, "steer")):
                res["_meta"] = {"steering": {"supported": True}}
        elif method == "session/new":
            log("mcp " + json.dumps(msg["params"].get("mcpServers", []), sort_keys=True))
            res = {"sessionId": "s-" + str(os.getpid()), "configOptions": config_options()}
        elif method == "session/load":
            if os.path.exists(os.path.join(DIR, "fail-load")):
                raise RuntimeError("session load failed")
            sid = msg["params"]["sessionId"]
            log(f"load {sid}")
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
