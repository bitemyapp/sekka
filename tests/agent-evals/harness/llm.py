"""A small OpenAI-compatible chat client (llama.cpp's llama-server, vLLM and
SGLang all serve this API), with the timings each server reports.

Standard library only: it runs inside the NixOS test driver's Python.
"""

import json
import time
import urllib.error
import urllib.request


class LLMError(Exception):
    pass


class Client:
    def __init__(self, base_url, model, api_key="none", timeout=900, params=None):
        self.base_url = base_url.rstrip("/")
        self.model = model
        self.api_key = api_key
        self.timeout = timeout
        # Request fields from the model's entry in models.toml: sampling,
        # chat_template_kwargs (enable_thinking), max_tokens.
        self.params = dict(params or {})

    def _post(self, path, body):
        data = json.dumps(body).encode()
        req = urllib.request.Request(
            self.base_url + path,
            data=data,
            headers={"Content-Type": "application/json", "Authorization": "Bearer " + self.api_key},
        )
        with urllib.request.urlopen(req, timeout=self.timeout) as r:
            return json.loads(r.read())

    def chat(self, messages, tools=None, retries=4, **params):
        body = {"model": self.model, "messages": messages, **self.params, **params}
        if tools:
            body["tools"] = tools
            body["tool_choice"] = "auto"
        delay = 5
        for attempt in range(retries + 1):
            start = time.monotonic()
            try:
                out = self._post("/chat/completions", body)
                out["_elapsed"] = time.monotonic() - start
                return out
            except urllib.error.HTTPError as e:
                text = e.read().decode(errors="replace")[:2000]
                # 400s are the request's fault (context overflow, a
                # template refusing the history): no point retrying.
                # A tool call the server couldn't parse comes back as a 500;
                # one retry samples again, then the agent loop takes over.
                if "parse tool call" in text and attempt >= 1:
                    raise LLMError(f"HTTP {e.code}: {text}") from e
                if 400 <= e.code < 500 or attempt == retries:
                    raise LLMError(f"HTTP {e.code}: {text}") from e
            except (urllib.error.URLError, TimeoutError, ConnectionError, OSError) as e:
                if attempt == retries:
                    raise LLMError(f"request failed: {e}") from e
            time.sleep(delay)
            delay *= 2
        raise LLMError("unreachable")


def usage_of(resp):
    """Token counts and server timings from one response, as plain numbers."""
    u = resp.get("usage") or {}
    t = resp.get("timings") or {}
    return {
        "prompt_tokens": u.get("prompt_tokens") or t.get("prompt_n") or 0,
        "completion_tokens": u.get("completion_tokens") or t.get("predicted_n") or 0,
        "cached_tokens": ((u.get("prompt_tokens_details") or {}).get("cached_tokens")) or t.get("cache_n") or 0,
        "elapsed": resp.get("_elapsed", 0.0),
        # llama-server reports its own prefill and decode times.
        "prompt_ms": t.get("prompt_ms"),
        "predicted_ms": t.get("predicted_ms"),
    }
