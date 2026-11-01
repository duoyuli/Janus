#!/usr/bin/env python3
"""A scripted ACP agent for orchestrator tests.

FAKE_DIR holds script.json, which maps a task id to the outcome of each successive prompt for it:
"ok", "fail", "cancelled", "slow", "write:<file>", "unmet" (reports its first acceptance criterion
unmet), "permission" (asks session/request_permission
with three options), "elicit" (asks elicitation/create with a form) or "ask" (ends the turn with a
question; the answer comes back as the next prompt). The last outcome repeats.
As a goal-mode lead it acknowledges the brief, then works on "/goal …" until FAKE_DIR/lead-go
exists and ends with the text of that file.
Session setup (session/new `_meta`, set_config_option) is appended to calls.log. Counters live in FAKE_DIR so the
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
    """Sleep, but stop early when the client sends session/cancel."""
    import time
    end = time.time() + seconds
    while time.time() < end:
        ready, _, _ = select.select([sys.stdin], [], [], 0.1)
        if ready:
            line = sys.stdin.readline()
            if not line:
                sys.exit(0)
            if json.loads(line).get("method") == "session/cancel":
                return True
    return False


def prompt(msg):
    sid = msg["params"]["sessionId"]
    text = msg["params"]["prompt"][0]["text"]
    if "以上是背景信息" in text:
        log("brief " + ("janus.orchestrate" in text and "ok" or "missing") + (" resumed" if "这是从中断处继续" in text else ""))
        say(sid, "已了解")
        return {"stopReason": "end_turn"}
    if text.startswith("/goal "):
        log("goal " + text[len("/goal "):])
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
            res = {"protocolVersion": 1, "agentInfo": {"name": "fake"}, "agentCapabilities": {}}
        elif method == "session/new":
            seed = (msg["params"].get("_meta") or {}).get("lody", {}).get("sessionConfig", {}).get("configOptionValues")
            if seed is not None:
                log("seed " + json.dumps(seed, sort_keys=True))
            res = {"sessionId": "s-" + str(os.getpid()), "configOptions": config_options()}
        elif method == "session/set_config_option":
            CONFIG[msg["params"]["configId"]] = msg["params"]["value"]
            log(f"set {msg['params']['configId']} {json.dumps(msg['params']['value'])}")
            res = {"configOptions": config_options()}
        elif method == "session/prompt":
            res = prompt(msg)
        else:
            res = {}
        send({"jsonrpc": "2.0", "id": mid, "result": res})
    except Exception as err:  # noqa: BLE001 - scripted failures are reported as JSON-RPC errors
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32000, "message": str(err)}})
