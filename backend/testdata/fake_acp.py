#!/usr/bin/env python3
"""A scripted ACP agent for orchestrator tests.

FAKE_DIR holds script.json, which maps a task id to the outcome of each successive prompt for it:
"ok", "fail", "cancelled", "slow", "write:<file>", "edit:<file>" (reports an edit of the file with ACP
`locations`, then stays busy for two seconds so tasks overlap), "unmet" (reports its first acceptance criterion
unmet), "permission" (asks session/request_permission
with three options), "elicit" (asks elicitation/create with a form), "loop" (makes the same tool call five times
before its result), "ask" (ends the turn with a question; the answer comes back as the next prompt) or "rich" (thinks,
publishes a plan and its slash commands, runs a tool that stays in progress for a moment and then reports a file diff,
reports its context usage and switches its own mode; the prompt response carries token usage). The last outcome repeats.
session/set_mode is logged ("mode <id>"), and the resource links of each prompt ("links <name>…").
With a `goal-sticky` file the lead reports its /goal active and never clears it; Janus's ask whether
it is done is answered "GOAL_DONE" with the lead-go text, and `_lody/session/goal` clear is logged ("goal clear").
As a goal-mode lead it acknowledges the brief, then works on "/goal …" until FAKE_DIR/lead-go
exists and ends with the text of that file.
With a `steer` file in FAKE_DIR it declares Lody steering: a `_lody/session/steer` that arrives during
a prompt is logged ("steer <text>"), answered "injected" and followed by `_lody/session/steer_applied`.
Session setup (session/new `_meta` and `mcpServers`, set_config_option) and the image blocks of each
prompt are appended to calls.log; it declares HTTP MCP support and image input. Counters live in FAKE_DIR so the
script carries across the separate processes of separate attempts.
"""

import json
import os
import re
import select
import sys

# @project-doc docs/operations/testing.md#fake_agent
DIR = os.environ["FAKE_DIR"]


def send(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def say(sid, text):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": sid, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}})


CRITERIA = {}  # session id -> acceptance criteria from the task prompt


def result(sid=None, unmet=False, **fields):
    """A structured result; every acceptance criterion of the session's task is checked (the first
    one reported unmet when `unmet`)."""
    checks = [{"criterion": c, "passed": not (unmet and i == 0), "evidence": "fake check" if not (unmet and i == 0) else "没做到"}
              for i, c in enumerate(CRITERIA.get(sid, []))]
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
            if msg.get("method") == "_lody/session/steer" and "id" in msg:
                p = msg["params"]
                log("steer " + " ".join(b.get("text", "") for b in p["prompt"]).replace("\n", " "))
                send({"jsonrpc": "2.0", "id": msg["id"], "result": {"outcome": "injected"}})
                send({"jsonrpc": "2.0", "method": "_lody/session/steer_applied", "params": {"sessionId": p["sessionId"], "steerId": p.get("steerId")}})
    return False


def prompt(msg):
    sid = msg["params"]["sessionId"]
    text = msg["params"]["prompt"][0]["text"]
    if "以上是背景信息" in text:
        log("brief " + ("janus.orchestrate" in text and "ok" or "missing") + (" resumed" if "这是从中断处继续" in text else ""))
        say(sid, "已了解")
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
                "sessionUpdate": "session_info_update", "_meta": {"lody": {"goal": {"objective": text[6:], "status": "active"}}}}}})
        import time
        go = os.path.join(DIR, "lead-go")
        while not os.path.exists(go):
            if wait_cancel(0.2):
                return {"stopReason": "cancelled"}
        say(sid, open(go).read())
        return {"stopReason": "end_turn"}
    m = re.search(r"你的任务 (\S+)：", text)
    task = m.group(1) if m else TASKS.get(sid, "unknown")
    TASKS[sid] = task
    c = re.search(r"验收标准：\n((?:- .*\n?)+)", text) if m else None
    if c:
        CRITERIA[sid] = [line[2:].strip() for line in c.group(1).splitlines() if line.startswith("- ")]
    outcome = next_outcome(task)
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
            if os.path.exists(os.path.join(DIR, "steer")):
                caps["_meta"] = {"lody": {"steering": {"version": 1, "transport": "request"}}}
            res = {"protocolVersion": 1, "agentInfo": {"name": "fake"}, "agentCapabilities": caps}
        elif method == "session/new":
            seed = (msg["params"].get("_meta") or {}).get("lody", {}).get("sessionConfig", {}).get("configOptionValues")
            if seed is not None:
                log("seed " + json.dumps(seed, sort_keys=True))
            log("mcp " + json.dumps(msg["params"].get("mcpServers", []), sort_keys=True))
            res = {"sessionId": "s-" + str(os.getpid()), "configOptions": config_options()}
        elif method == "session/set_config_option":
            CONFIG[msg["params"]["configId"]] = msg["params"]["value"]
            log(f"set {msg['params']['configId']} {json.dumps(msg['params']['value'])}")
            res = {"configOptions": config_options()}
        elif method == "_lody/session/goal":
            log(f"goal {msg['params']['action']}")
            send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": msg["params"]["sessionId"], "update": {
                "sessionUpdate": "session_info_update", "_meta": {"lody": {"goal": None}}}}})
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
