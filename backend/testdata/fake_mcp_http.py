#!/usr/bin/env python3
"""A scripted MCP server over HTTP for the MCP router tests: Streamable HTTP at /mcp and the older
SSE transport at /sse. It listens on a free port of 127.0.0.1 and prints the port on its first line.

Every request must carry `Authorization: Bearer good`, or it gets 401. Each event goes as a line to
the log named by the first argument: `init` (a new Streamable HTTP session), `delete` (a session
ended by the client), `sse` (an SSE stream opened). Tools: `whoami` answers the Authorization and
MCP-Protocol-Version headers it was called with; `expire` forgets the Streamable HTTP session, so
the next request with it gets 404. Over Streamable HTTP a tool call is answered as an event stream,
after a notification, and everything else as JSON.
"""

import json
import queue
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

# @project-doc docs/operations/20261031-testing.md#fake_agent
LOG = sys.argv[1]
LOCK = threading.Lock()
STATE = {"session": None, "count": 0}
STREAMS = {}

TOOLS = [
    {"name": "whoami", "description": "返回调用时的鉴权和协议版本 header", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "expire", "description": "忘掉当前会话", "inputSchema": {"type": "object", "properties": {}}},
]


def log(line):
    with LOCK, open(LOG, "a") as f:
        f.write(line + "\n")


def answer(msg, headers):
    """The reply to one request, or None for a notification or a response."""
    mid, method = msg.get("id"), msg.get("method")
    if mid is None or method is None:
        return None
    if method == "initialize":
        result = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake-http", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": TOOLS}
    elif method == "tools/call":
        name = msg["params"]["name"]
        if name == "whoami":
            seen = {"authorization": headers.get("Authorization"), "version": headers.get("MCP-Protocol-Version")}
            result = {"content": [{"type": "text", "text": json.dumps(seen)}]}
        elif name == "expire":
            STATE["session"] = None
            result = {"content": [{"type": "text", "text": "expired"}]}
        else:
            return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32602, "message": f"no tool {name}"}}
    else:
        return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": method}}
    return {"jsonrpc": "2.0", "id": mid, "result": result}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def allowed(self):
        if self.headers.get("Authorization") == "Bearer good":
            return True
        self.reply(401, b"unauthorized")
        return False

    def reply(self, code, body=b"", kind="text/plain", extra=None):
        self.send_response(code)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def body(self):
        return json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"null")

    def do_POST(self):
        if not self.allowed():
            return
        url = urlparse(self.path)
        msg = self.body()
        if url.path == "/messages":
            out = STREAMS.get(parse_qs(url.query).get("sid", [""])[0])
            if out is None:
                return self.reply(404)
            reply = answer(msg, self.headers)
            if reply:
                out.put(reply)
            return self.reply(202)
        if msg.get("method") == "initialize":
            STATE["count"] += 1
            STATE["session"] = f"s{STATE['count']}"
            log("init")
            return self.reply(200, json.dumps(answer(msg, self.headers)).encode(), "application/json", {"Mcp-Session-Id": STATE["session"]})
        sid = self.headers.get("Mcp-Session-Id")
        if sid != STATE["session"]:
            return self.reply(404 if sid else 400)
        reply = answer(msg, self.headers)
        if reply is None:
            return self.reply(202)
        if msg.get("method") == "tools/call":
            note = {"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info", "data": "working"}}
            stream = f": hello\n\nevent: message\ndata: {json.dumps(note)}\n\ndata: {json.dumps(reply)}\n\n".encode()
            return self.reply(200, stream, "text/event-stream")
        self.reply(200, json.dumps(reply).encode(), "application/json")

    def do_DELETE(self):
        if not self.allowed():
            return
        if self.headers.get("Mcp-Session-Id") == STATE["session"]:
            STATE["session"] = None
            log("delete")
        self.reply(200)

    def do_GET(self):
        if not self.allowed():
            return
        if urlparse(self.path).path != "/sse":
            return self.reply(405)
        sid = f"x{len(STREAMS) + 1}"
        out = queue.Queue()
        STREAMS[sid] = out
        log("sse")
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            self.wfile.write(f"event: endpoint\ndata: /messages?sid={sid}\n\n".encode())
            self.wfile.flush()
            while True:
                msg = out.get()
                self.wfile.write(f"event: message\ndata: {json.dumps(msg)}\n\n".encode())
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
server.daemon_threads = True
print(server.server_address[1], flush=True)
server.serve_forever()
