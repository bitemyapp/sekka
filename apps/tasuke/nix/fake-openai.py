# SPDX-License-Identifier: MIT OR Apache-2.0
"""A scripted OpenAI-compatible chat server for Tasuke's VM test.

Every chat request that offers tools gets the next step of a fixed script,
streamed as server-sent events the way llama-server streams them: a
read-only run_command, a run_command that needs approval, a run_as_root,
a run_command Tasuke refuses, then a final answer. The step is the number
of assistant tool-call turns already in the conversation, so the script
replays the same way for every conversation. Requests without tools (a
title or a summary) get a short plain answer.

Each request body is appended to LOG as a JSON line. It also answers
/health, /models and /props, so it can stand in for llama-server.

    python3 -I fake-openai.py PORT LOG
"""

import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
LOG = sys.argv[2]

STEPS = [
    ("run_command", {"command": "cat /etc/tasuke-test/marker", "purpose": "Read the test's marker file."}),
    ("run_command", {"command": "touch /tmp/tasuke-test/approved", "purpose": "Make a file, which needs approval."}),
    (
        "run_as_root",
        {"command": "date > /var/lib/tasuke-test/root-ran", "purpose": "Write a file only root can write."},
    ),
    ("run_command", {"command": "rm -rf ~", "purpose": "A command Tasuke must refuse."}),
]
ANSWER = "All four steps are done."


def chunk(delta, finish=None, usage=None):
    c = {
        "id": "chatcmpl-fake",
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "model": "fake",
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
    }
    if usage:
        c["usage"] = usage
    return c


USAGE = {"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120}


def reply(body):
    """The step's (tool call or None, text) for this request."""
    if not body.get("tools"):
        return None, "Tasuke test"
    done = sum(1 for m in body.get("messages", []) if m.get("role") == "assistant" and m.get("tool_calls"))
    if done < len(STEPS):
        name, args = STEPS[done]
        return {"id": f"call_{done + 1}", "name": name, "arguments": json.dumps(args)}, None
    return None, ANSWER


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def log_message(self, fmt, *args):
        sys.stderr.write("fake-openai: " + (fmt % args) + "\n")

    def json(self, status, value):
        data = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path in ("/health", "/v1/health"):
            self.json(200, {"status": "ok"})
        elif path in ("/models", "/v1/models"):
            self.json(200, {"object": "list", "data": [{"id": "fake", "object": "model", "status": {"value": "loaded"}}]})
        elif path in ("/props", "/v1/props"):
            self.json(200, {"default_generation_settings": {"n_ctx": 32768}, "n_ctx": 32768, "is_sleeping": False})
        else:
            self.json(404, {"error": {"message": f"no {path} here"}})

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length)
        try:
            body = json.loads(raw or b"{}")
        except ValueError:
            self.json(400, {"error": {"message": "not JSON"}})
            return
        with open(LOG, "a") as log:
            log.write(json.dumps({"path": self.path, "body": body}) + "\n")
        if not self.path.split("?")[0].endswith("/chat/completions"):
            self.json(404, {"error": {"message": f"no {self.path} here"}})
            return
        call, text = reply(body)
        if not body.get("stream"):
            message = {"role": "assistant", "content": text}
            if call:
                message["tool_calls"] = [
                    {"id": call["id"], "type": "function", "function": {"name": call["name"], "arguments": call["arguments"]}}
                ]
            self.json(
                200,
                {
                    "id": "chatcmpl-fake",
                    "object": "chat.completion",
                    "created": int(time.time()),
                    "model": "fake",
                    "choices": [{"index": 0, "message": message, "finish_reason": "tool_calls" if call else "stop"}],
                    "usage": USAGE,
                },
            )
            return
        if call:
            chunks = [
                chunk(
                    {
                        "role": "assistant",
                        "content": None,
                        "tool_calls": [
                            {"index": 0, "id": call["id"], "type": "function", "function": {"name": call["name"], "arguments": ""}}
                        ],
                    }
                ),
                chunk({"tool_calls": [{"index": 0, "function": {"arguments": call["arguments"]}}]}),
                chunk({}, "tool_calls", USAGE),
            ]
        else:
            half = len(text) // 2
            chunks = [
                chunk({"role": "assistant", "content": text[:half]}),
                chunk({"content": text[half:]}),
                chunk({}, "stop", USAGE),
            ]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        for c in chunks:
            self.wfile.write(b"data: " + json.dumps(c).encode() + b"\n\n")
            self.wfile.flush()
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
