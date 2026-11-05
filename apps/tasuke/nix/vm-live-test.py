"""The window on Aizome answering through a real model on the host."""
import os

QUESTION = "How much disk space is free here, and what is using the most memory right now?"
MODEL = os.environ.get("TASUKE_LIVE_MODEL", "gemma-4-26b-a4b")
KEY = os.environ["TASUKE_LIVE_KEY"]
catalog = json.load(open(CATALOG))
SESSIONS = "/home/alice/.local/share/tasuke/sessions"


def records():
    return [json.loads(line) for line in user(f"cat {SESSIONS}/*.jsonl").splitlines() if line.strip()]


def events():
    return [r["event"] for r in records() if r["kind"] == "event"]


start_session()

with subtest("the window opens on the model the host serves, with the host server's key"):
    # The model's files at their catalog sizes, sparse: the window counts it
    # as downloaded, and the host serves the real one under the same name.
    user(f"mkdir -p ~/.config/tasuke ~/.local/share/tasuke/models/{MODEL}")
    user(f"printf '[model]\\nid = \"{MODEL}\"\\n' > ~/.config/tasuke/config.toml")
    for f in catalog[MODEL]["files"]:
        user(f"truncate -s {f['size']} ~/.local/share/tasuke/models/{MODEL}/{f['path'].rsplit('/', 1)[-1]}")
    # Copied, not typed, so the key stays out of the log.
    machine.copy_from_host(KEY, "/run/user/1000/tasuke/api-key")
    machine.succeed("chown alice /run/user/1000/tasuke/api-key && chmod 600 /run/user/1000/tasuke/api-key")
    user("aizomectl spawn aizome-tasuke")
    click("the composer", lambda e: e.get("role") == "multilinetextinput")
    input_("type " + shlex.quote(QUESTION))
    input_("key Return")
    asked = time.monotonic()

with subtest("a real model looks, then answers, asking for nothing"):
    until("the turn started", lambda: find(button("Stop")))
    # A cold start loads the model on the host first.
    until("the turn ended", lambda: not find(button("Stop")), timeout=900)
    took = time.monotonic() - asked
    machine.screenshot("answered")
    ev = events()
    errors = [e["text"] for e in ev if e["type"] == "error"]
    assert not errors, errors
    calls = [e for e in ev if e["type"] == "tool_call"]
    assert calls, "it looked before answering"
    asks = [e["call"] for e in calls if e["needs"]["kind"] != "none"]
    assert not asks, f"it asked to change something: {asks}"
    answer = "".join(e["text"] for e in ev if e["type"] == "text_delta").strip()
    assert answer, "an answer"
    finished = [e for e in ev if e["type"] == "turn_finished"]
    assert finished, "the turn finished"
    tps = finished[-1].get("decode_tps")
    # What it saw beside what it said, for a person to compare.
    outputs = {e["id"]: e["output"]["text"] for e in ev if e["type"] == "tool_output"}
    for c in calls:
        print(f"$ {c['call'].get('command', json.dumps(c['call']))}\n{outputs.get(c['id'], '(no output)')}")
    print(f"{MODEL} looked {len(calls)} times and answered in {took:.1f} s at {tps} tok/s:\n{answer}")
