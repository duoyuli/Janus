#!/usr/bin/env python3
"""A scripted MCP server over stdio for the MCP router tests.

Each start appends "start" to FAKE_MCP_LOG, so a test can tell whether listing tools started it.
Tools: `cwd` answers the directory it runs in; `echo` answers its `text`; `roots` asks the client
`roots/list` and answers the first root's URI, so a test sees the router answer the server's own
requests.
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
        else:
            send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32602, "message": f"no tool {name}"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": method}})
