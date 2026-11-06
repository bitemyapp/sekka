"""Records what Tasuke sends a model, for tasuke_sft.py: every chat
request's body, appended to LOG as a JSON line, answered at once with
"Done." and no tool calls, so the agent runs nothing. Streams the way
llama-server does (after apps/tasuke/nix/fake-openai.py). Point one
training fault at it, e.g.

    python3 -I tasuke_capture.py 8159 capture.jsonl &
    run.py --agent tasuke --set train --models gemma-4-31b --no-serve --port 8159 \
        --scenarios journal-not-stored --track diagnostics --repeats 1

The first line's system prompt and tools are then the ones Tasuke gives
every model, which tasuke_sft.py puts in front of each conversation.
"""

import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT, LOG = int(sys.argv[1]), sys.argv[2]
USAGE = {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}


def chunk(delta, finish=None):
    c = {"id": "x", "object": "chat.completion.chunk", "created": int(time.time()), "model": "capture",
         "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
    if finish:
        c["usage"] = USAGE
    return c


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def log_message(self, fmt, *args):
        pass

    def send_json(self, value):
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path.endswith("/models"):
            self.send_json({"object": "list", "data": [{"id": "capture", "object": "model"}]})
        elif path.endswith("/props"):
            self.send_json({"default_generation_settings": {"n_ctx": 32768}, "n_ctx": 32768})
        else:
            self.send_json({"status": "ok"})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)) or b"{}")
        with open(LOG, "a") as f:
            f.write(json.dumps({"path": self.path, "body": body}) + "\n")
        if not body.get("stream"):
            self.send_json({"id": "x", "object": "chat.completion", "created": int(time.time()), "model": "capture",
                            "choices": [{"index": 0, "message": {"role": "assistant", "content": "Done."},
                                         "finish_reason": "stop"}], "usage": USAGE})
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for c in (chunk({"role": "assistant", "content": "Done."}), chunk({}, "stop")):
            self.wfile.write(b"data: " + json.dumps(c).encode() + b"\n\n")
        self.wfile.write(b"data: [DONE]\n\n")


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
