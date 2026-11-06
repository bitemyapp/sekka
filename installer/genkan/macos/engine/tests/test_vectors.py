# SPDX-License-Identifier: GPL-3.0-or-later
"""The committed transcripts are what the engine writes today, byte for byte.

crates/contract/tests/engine_vectors.rs feeds the same files to the Rust
`Transcript` the helper uses; together the two prove that the helper accepts
what this engine writes. When SEKKA_TRANSCRIPT_VALIDATOR names the
contract's validate_transcript example, the fresh transcripts are checked
with it here too.
"""

import json
import logging
import os
import subprocess
import unittest

import scenarios
import support as S
from sekka_engine import canonical

logging.disable(logging.CRITICAL)


def validate(lines, meta):
    validator = os.environ.get("SEKKA_TRANSCRIPT_VALIDATOR")
    if not validator:
        return None
    args = [validator, meta["mode"]]
    if meta["mode"] == "execute":
        args += [meta["plan"], "shrinks" if meta["shrinks"] else "no-shrink"]
    p = subprocess.run(args, input=b"".join(line + b"\n" for line in lines), capture_output=True)
    return p.returncode, p.stdout.decode().strip()


class TranscriptVectors(unittest.TestCase):
    def setUp(self):
        with open(os.path.join(S.VECTORS, "transcripts.json")) as f:
            self.meta = json.load(f)
        self.t = S.TempDir()

    def tearDown(self):
        self.t.cleanup()

    def check(self, name, lines, plan=None):
        meta = self.meta[name]
        if plan is not None:
            self.assertEqual(canonical.digest(plan), meta["plan"])
        self.assertEqual(lines, scenarios.read_lines(os.path.join(S.VECTORS, name)), name)
        result = validate(lines, meta)
        if result is not None:
            self.assertEqual(result, (0, f"accepted: {meta['outcome']}"), name)

    def test_inspect(self):
        *_, channel = scenarios.clean_inspection(self.t)
        self.check("inspect-j514s-clean.jsonl", channel.lines)

    def test_happy_path(self):
        r = scenarios.execute(self.t)
        self.check("execute-happy.jsonl", r["channel"].lines, r["plan"])

    def test_free_space(self):
        r = scenarios.execute(self.t, builder=S.j514s_free_space, place="free-space", sekka=150 * S.GIB)
        self.check("execute-free-space.jsonl", r["channel"].lines, r["plan"])

    def test_failures(self):
        for name, before in (
            ("execute-resize-69606.jsonl", scenarios.resize_fails),
            ("execute-layout-changed.jsonl", scenarios.layout_changes),
            ("execute-root-read-back.jsonl", scenarios.read_back_corrupts),
        ):
            with self.subTest(name):
                self.t.cleanup()
                self.t = S.TempDir()
                r = scenarios.execute(self.t, before=before)
                self.check(name, r["channel"].lines, r["plan"])


if __name__ == "__main__":
    unittest.main()
