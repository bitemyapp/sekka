# SPDX-License-Identifier: GPL-3.0-or-later
"""The engine's transcript: one canonical `sekka.mac.engine/1` message per
line on its own output, numbered from 1, and the helper's `{"ack":SEQ}`
answers to intents on its own input (engine.rs)."""

import logging

from . import canonical
from . import contract as c

log = logging.getLogger("sekka_engine.transcript")


class AckError(RuntimeError):
    """The helper didn't acknowledge an intent as the protocol requires."""


class TranscriptWriter:
    def __init__(self, out, acks=None):
        self.out = out
        self.acks = acks
        self.seq = 0
        self.lines = []

    def send(self, kind, payload):
        self.seq += 1
        message = c.engine_message(self.seq, kind, payload)
        line = canonical.dumps(message)
        if len(line) > c.LIMIT_ENGINE_LINE:
            raise ValueError("an engine message is larger than the contract allows")
        self.lines.append(line)
        try:
            self.out.write(line + b"\n")
            if hasattr(self.out, "flush"):
                self.out.flush()
        except (BrokenPipeError, OSError) as error:
            log.error("the transcript can't be written: %s", error)
            raise
        return self.seq

    def hello(self, engine, asahi, adapter, mode):
        return self.send(
            "hello",
            {"engine": engine, "asahi": asahi, "adapter": adapter, "mode": mode, "protocol": c.ENGINE_PROTOCOL},
        )

    def inspection(self, doc):
        return self.send("inspection", doc)

    def intent(self, stage, plan):
        return self.send("intent", {"stage": stage, "plan": plan})

    def checkpoint(self, stage, plan, facts, verification):
        if verification not in c.VERIFICATIONS:
            raise ValueError(verification)
        return self.send("checkpoint", {"stage": stage, "plan": plan, "facts": facts, "verification": verification})

    def progress(self, stage, done, total):
        return self.send("progress", {"stage": stage, "done": int(min(done, total)), "total": int(total)})

    def log(self, level, text):
        return self.send("log", {"level": level, "text": c.text(c.plain_text(text, keep="start"))})

    def failure(self, stage, failure):
        return self.send("failure", {"stage": stage, "failure": failure})

    def completion(self, outcome):
        if outcome not in c.OUTCOMES:
            raise ValueError(outcome)
        return self.send("completion", {"outcome": outcome})

    def wait_ack(self, seq):
        """Block until the helper answers `{"ack":seq}`: its journal holds
        the intent. Anything else, or the end of input, stops the engine
        before the stage changes anything."""
        if self.acks is None:
            raise AckError("no acknowledgement channel")
        line = bytearray()
        while True:
            b = self.acks.read(1)
            if not b:
                raise AckError("the helper closed the acknowledgement channel")
            if b == b"\n":
                break
            line.extend(b)
            if len(line) > 64:
                raise AckError("an acknowledgement is longer than any the protocol allows")
        expected = canonical.dumps({"ack": seq})
        if bytes(line) != expected:
            raise AckError(f"expected {expected.decode()}, the helper answered {bytes(line)[:64]!r}")
