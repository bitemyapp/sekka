"""The agent loop: a model, a set of tools, a step and time budget.

The model acts through OpenAI-style tool calls. Models that write a call
as text instead (`<tool_call>{...}</tool_call>`, a fenced JSON object)
still get it run, and the transcript counts those, since an agent built on
a stricter client would have failed there.
"""

import json
import re
import time
import uuid

from llm import LLMError, usage_of

FINISH = "finish"


class Tools:
    """What the agent may do. `call` returns (text, images); images are
    PNG bytes, sent to the model after the tool results."""

    def schemas(self):
        raise NotImplementedError

    def call(self, name, args):
        raise NotImplementedError


def function(name, description, properties, required=()):
    return {
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {"type": "object", "properties": properties, "required": list(required)},
        },
    }


TOOL_CALL_TAG = re.compile(r"<tool_call>\s*(\{.*?\})\s*</tool_call>", re.S)
FENCED_JSON = re.compile(r"```(?:json)?\s*(\{.*?\})\s*```", re.S)


def text_tool_calls(content, names):
    """Tool calls written in the message text, as some templates leave them."""
    if not content:
        return []
    found = []
    for pattern in (TOOL_CALL_TAG, FENCED_JSON):
        for m in pattern.finditer(content):
            try:
                obj = json.loads(m.group(1))
            except json.JSONDecodeError:
                continue
            name = obj.get("name") or obj.get("function")
            args = obj.get("arguments", obj.get("parameters", {}))
            if name in names:
                found.append({"name": name, "arguments": args})
        if found:
            return found
    # A bare JSON object as the whole message.
    s = content.strip()
    if s.startswith("{") and s.endswith("}"):
        try:
            obj = json.loads(s)
            if obj.get("name") in names:
                return [{"name": obj["name"], "arguments": obj.get("arguments", {})}]
        except json.JSONDecodeError:
            pass
    return []


def image_part(png):
    import base64

    return {"type": "image_url", "image_url": {"url": "data:image/png;base64," + base64.b64encode(png).decode()}}


class Transcript:
    def __init__(self, path):
        self.f = open(path, "a")

    def write(self, kind, **data):
        self.f.write(json.dumps({"t": time.time(), "kind": kind, **data}, default=str) + "\n")
        self.f.flush()


def drop_old_images(messages, keep):
    """Keep only the newest `keep` images: screenshots dominate the context."""
    seen = 0
    for m in reversed(messages):
        if isinstance(m.get("content"), list):
            parts = []
            for p in reversed(m["content"]):
                if p.get("type") == "image_url":
                    seen += 1
                    if seen > keep:
                        p = {"type": "text", "text": "[an earlier screenshot, no longer shown]"}
                parts.append(p)
            m["content"] = list(reversed(parts))


OBSERVATION_MARK = "Windows and their controls"


def drop_old_observations(messages, keep):
    """Element lists are as long as screenshots in tokens: keep the newest."""
    seen = 0
    for m in reversed(messages):
        c = m.get("content")
        if m.get("role") == "tool" and isinstance(c, str) and OBSERVATION_MARK in c:
            seen += 1
            if seen > keep:
                m["content"] = c.split(OBSERVATION_MARK)[0] + "[an earlier element list, no longer shown]"


def run(client, system, task, tools, transcript, max_steps=40, max_seconds=1200, keep_images=3, max_tokens=4096,
        keep_reasoning=False, images_in=None):
    """Run until the model calls `finish`, runs out of steps or time, or the
    server fails. Returns a dict describing how it ended."""
    names = {t["function"]["name"] for t in tools.schemas()}
    first = [{"type": "text", "text": task}]
    if images_in:
        first += [image_part(png) for png in images_in]
    messages = [{"role": "system", "content": system}, {"role": "user", "content": first if images_in else task}]
    transcript.write("start", system=system, task=task, tools=tools.schemas())
    stats = {
        "steps": 0,
        "tool_calls": 0,
        "text_tool_calls": 0,
        "bad_arguments": 0,
        "unknown_tools": 0,
        "nudges": 0,
        "prompt_tokens": 0,
        "completion_tokens": 0,
        "llm_seconds": 0.0,
        "max_prompt_tokens": 0,
    }
    start = time.monotonic()
    result = {"ended": "max-steps", "final": None}
    nudges_in_a_row = 0
    for step in range(max_steps):
        if time.monotonic() - start > max_seconds:
            result["ended"] = "time"
            break
        drop_old_images(messages, keep_images)
        drop_old_observations(messages, keep_images)
        try:
            resp = client.chat(messages, tools=tools.schemas(), max_tokens=max_tokens)
        except LLMError as e:
            transcript.write("llm-error", error=str(e))
            result["ended"] = "llm-error"
            result["error"] = str(e)[:500]
            break
        stats["steps"] += 1
        u = usage_of(resp)
        stats["prompt_tokens"] += u["prompt_tokens"]
        stats["completion_tokens"] += u["completion_tokens"]
        stats["max_prompt_tokens"] = max(stats["max_prompt_tokens"], u["prompt_tokens"])
        stats["llm_seconds"] += u["elapsed"]
        choice = resp["choices"][0]
        msg = choice.get("message") or {}
        content = msg.get("content") or ""
        reasoning = msg.get("reasoning_content") or msg.get("reasoning") or ""
        calls = []
        for tc in msg.get("tool_calls") or []:
            fn = tc.get("function") or {}
            calls.append({"id": tc.get("id") or "call_" + uuid.uuid4().hex[:8], "name": fn.get("name"),
                          "arguments": fn.get("arguments"), "native": True})
        if not calls:
            for c in text_tool_calls(content, names):
                calls.append({"id": "call_" + uuid.uuid4().hex[:8], "name": c["name"], "arguments": c["arguments"],
                              "native": False})
        transcript.write("assistant", step=step, content=content, reasoning=reasoning,
                         tool_calls=[{k: c[k] for k in ("name", "arguments", "native")} for c in calls],
                         finish_reason=choice.get("finish_reason"), usage=u)
        native = [c for c in calls if c["native"]]
        assistant = {"role": "assistant", "content": content}
        if keep_reasoning and reasoning:
            assistant["reasoning_content"] = reasoning
        if native:
            assistant["tool_calls"] = [
                {"id": c["id"], "type": "function",
                 "function": {"name": c["name"], "arguments": c["arguments"] if isinstance(c["arguments"], str)
                              else json.dumps(c["arguments"])}}
                for c in native
            ]
        messages.append(assistant)
        if not calls:
            nudges_in_a_row += 1
            stats["nudges"] += 1
            if nudges_in_a_row > 2:
                result["ended"] = "no-action"
                result["final"] = {"text": content}
                break
            messages.append({"role": "user", "content":
                             "Continue by calling one of the tools. When you are done, call `finish`."})
            continue
        nudges_in_a_row = 0
        images = []
        text_results = []
        finished = None
        for c in calls:
            stats["tool_calls"] += 1
            if not c["native"]:
                stats["text_tool_calls"] += 1
            args = c["arguments"]
            if isinstance(args, str):
                try:
                    args = json.loads(args) if args.strip() else {}
                except json.JSONDecodeError:
                    args = None
            if not isinstance(args, dict):
                stats["bad_arguments"] += 1
                out = f"Error: the arguments of {c['name']} were not a JSON object: {c['arguments']!r}"
                imgs = []
            elif c["name"] not in names:
                stats["unknown_tools"] += 1
                out = f"Error: there is no tool named {c['name']!r}. The tools are: {', '.join(sorted(names))}."
                imgs = []
            elif c["name"] == FINISH:
                finished = args
                out = "Finished."
                imgs = []
            else:
                t0 = time.monotonic()
                try:
                    out, imgs = tools.call(c["name"], args)
                except Exception as e:  # noqa: BLE001 (a tool's failure is the model's to see)
                    out, imgs = f"Error: {type(e).__name__}: {e}", []
                transcript.write("tool", step=step, name=c["name"], arguments=args, output=out,
                                 images=len(imgs), seconds=round(time.monotonic() - t0, 2))
            images += imgs
            if c["native"]:
                messages.append({"role": "tool", "tool_call_id": c["id"], "content": out})
            else:
                text_results.append(f"Result of {c['name']}:\n{out}")
        if text_results:
            messages.append({"role": "user", "content": "\n\n".join(text_results)})
        if finished is not None:
            result["ended"] = "finished"
            result["final"] = finished
            break
        if images:
            messages.append({"role": "user", "content": [{"type": "text", "text": "The screen now:"}]
                             + [image_part(p) for p in images[-1:]]})
    stats["seconds"] = round(time.monotonic() - start, 1)
    result["stats"] = stats
    transcript.write("end", **result)
    return result
