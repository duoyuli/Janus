#!/usr/bin/env python3
"""A scripted MCP server over stdio for the MCP router tests.

Each start appends "start" to FAKE_MCP_LOG, so a test can tell whether listing tools started it.
Tools: `cwd` answers the directory it runs in; `echo` answers its `text`; `roots` asks the client
`roots/list` and answers the first root's URI, so a test sees the router answer the server's own
requests; `env` answers HOME and the variable `name` (null when unset), so a test sees what
environment it was started in; `grow` adds a tool `extra` and says its tools changed; `quit` answers,
then exits.
"""

import json
import os
import sys

# @project-doc docs/operations/20261031-testing.md#fake_agent
LOG = os.environ.get("FAKE_MCP_LOG")
if LOG:
    with open(LOG, "a") as f:
        f.write("start\n")

TOOLS = [
    {"name": "cwd", "description": "返回服务器的工作目录", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "echo", "description": "原样返回 text", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}},
    {"name": "roots", "description": "向客户端要 roots/list", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "env", "description": "返回 HOME 和 name 指定的环境变量", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}}},
    {"name": "grow", "description": "新增工具 extra 并通知客户端", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "quit", "description": "回答后退出", "inputSchema": {"type": "object", "properties": {}}},
]


def send(msg):
    sys.stdout.write(json.dumps(msg, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def text(s):
    return {"content": [{"type": "text", "text": s}]}


for line in sys.stdin:
    msg = json.loads(line)
    method, mid = msg.get("method"), msg.get("id")
    if mid is None:
        continue
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                                                       "serverInfo": {"name": "fake", "version": "1"}}})
    elif method == "tools/list":
        send({"jsonrpc": "2.0", "id": mid, "result": {"tools": TOOLS}})
    elif method == "tools/call":
        name, args = msg["params"]["name"], msg["params"].get("arguments") or {}
        if name == "cwd":
            send({"jsonrpc": "2.0", "id": mid, "result": text(os.getcwd())})
        elif name == "echo":
            send({"jsonrpc": "2.0", "id": mid, "result": text(args.get("text", ""))})
        elif name == "roots":
            send({"jsonrpc": "2.0", "id": "r1", "method": "roots/list", "params": {}})
            reply = json.loads(sys.stdin.readline())
            send({"jsonrpc": "2.0", "id": mid, "result": text(reply["result"]["roots"][0]["uri"])})
        elif name == "grow":
            TOOLS.append({"name": "extra", "description": "后来加的工具", "inputSchema": {"type": "object", "properties": {}}})
            send({"jsonrpc": "2.0", "id": mid, "result": text("grown")})
            send({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
        elif name == "quit":
            send({"jsonrpc": "2.0", "id": mid, "result": text("bye")})
            sys.exit(0)
        elif name == "env":
            seen = {"HOME": os.environ.get("HOME"), args.get("name", ""): os.environ.get(args.get("name", ""))}
            send({"jsonrpc": "2.0", "id": mid, "result": text(json.dumps(seen))})
        else:
            send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32602, "message": f"no tool {name}"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": method}})
