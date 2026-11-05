#!/usr/bin/env python3
"""Behavioral fixtures for evaluate-captions.py; no network, models, or audio truth.

Run: python3 desktop/aizome/scripts/test-evaluate-captions.py
These invented strings exercise scoring/parsing, not caption quality claims.
"""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("evaluate_captions", Path(__file__).with_name("evaluate-captions.py"))
EVALUATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVALUATOR)


def json3(events):
    return json.dumps({"events": events})


def timed_event(start=0, words=("hello", " world"), duration=2000, append=0):
    return {"tStartMs": start, "dDurationMs": duration, "wWinId": 1, "aAppend": append,
            "segs": [{"utf8": value, "tOffsetMs": index * 500} for index, value in enumerate(words)]}


class ScoringTests(unittest.TestCase):
    def test_normalization_is_explicit_and_keeps_repetitions(self):
        self.assertEqual(EVALUATOR.normalize("ＨＥＬＬＯ, hello! Don’t stop—42."),
                         ["hello", "hello", "don't", "stop", "42"])

    def test_substitution_deletion_insertion_counts(self):
        for reference, hypothesis, expected in [
            ("a b c", "a x c", (1, 0, 0)),
            ("a b c", "a c", (0, 1, 0)),
            ("a b c", "a b c d", (0, 0, 1)),
            ("a b c", "", (0, 3, 0)),
        ]:
            with self.subTest(reference=reference, hypothesis=hypothesis):
                score = EVALUATOR.wer(reference.split(), hypothesis.split())
                self.assertEqual(tuple(score[field] for field in ("substitutions", "deletions", "insertions")), expected)
                self.assertEqual(score["errors"], sum(expected))
                self.assertEqual(score["wer"], sum(expected) / 3)

    def test_wer_can_exceed_one(self):
        self.assertEqual(EVALUATOR.wer(["a"], ["x", "y", "z"])["wer"], 3)


class CaptionParsingTests(unittest.TestCase):
    def test_json3_timestamped_replay_removed(self):
        event = timed_event()
        value, evidence = EVALUATOR.read_json3(json3([event, copy.deepcopy(event)]), (0, 5))
        self.assertEqual(EVALUATOR.normalize(value), ["hello", "world"])
        self.assertEqual(evidence["timed_replays_removed"], 2)

    def test_json3_genuine_repeated_phrase_preserved(self):
        value, _ = EVALUATOR.read_json3(json3([timed_event(), timed_event(1000)]), (0, 5))
        self.assertEqual(EVALUATOR.normalize(value), ["hello", "world", "hello", "world"])

    def test_json3_single_asr_word_uses_implicit_zero_offset(self):
        singleton = {"tStartMs": 1000, "dDurationMs": 4000, "wWinId": 1,
                     "segs": [{"utf8": "hello", "acAsrConf": 0}]}
        value, _ = EVALUATOR.read_json3(json3([
            timed_event(), singleton, timed_event(1500, words=("again", " hello")),
        ]), (0, 3))
        self.assertEqual(EVALUATOR.normalize(value), ["hello", "world", "hello", "again", "hello"])

    def test_json3_unrelated_revisions_outside_clip_do_not_invalidate_words(self):
        before = timed_event(0)
        conflicting = timed_event(0, words=("different", " outside"))
        selected = timed_event(5000, words=("inside", " clip"))
        after = {"tStartMs": 10000, "dDurationMs": 5000,
                 "segs": [{"utf8": "untimed outside"}]}
        value, _ = EVALUATOR.read_json3(json3([before, conflicting, selected, after]), (5, 7))
        self.assertEqual(EVALUATOR.normalize(value), ["inside", "clip"])

    def test_json3_explicit_append_is_not_lexical_deduplication(self):
        first = {"tStartMs": 0, "dDurationMs": 2000, "wWinId": 1, "segs": [{"utf8": "go go"}]}
        second = {"tStartMs": 1000, "dDurationMs": 2000, "wWinId": 1,
                  "aAppend": 1, "segs": [{"utf8": " go go"}]}
        value, _ = EVALUATOR.read_json3(json3([first, second]), (0, 5))
        self.assertEqual(EVALUATOR.normalize(value), ["go"] * 4)

    def test_json3_ambiguous_untimed_replacement_rejected(self):
        first = {"tStartMs": 0, "dDurationMs": 2000, "segs": [{"utf8": "hello world"}]}
        second = {"tStartMs": 1000, "dDurationMs": 2000, "segs": [{"utf8": "hello world again"}]}
        with self.assertRaisesRegex(EVALUATOR.EvidenceError, "overlapping untimed"):
            EVALUATOR.read_json3(json3([first, second]), (0, 5))

    def test_json3_correction_is_not_silently_deduplicated(self):
        with self.assertRaisesRegex(EVALUATOR.EvidenceError, "conflicting caption revisions"):
            EVALUATOR.read_json3(json3([timed_event(), timed_event(words=("goodbye", " world"))]), (0, 5))

    def test_json3_newline_append_is_display_only(self):
        newline = {"tStartMs": 800, "aAppend": 1, "segs": [{"utf8": "\n"}]}
        value, _ = EVALUATOR.read_json3(json3([timed_event(), newline, timed_event(1000)]), (0, 5))
        self.assertEqual(EVALUATOR.normalize(value), ["hello", "world", "hello", "world"])

    def test_json3_clip_is_start_inclusive_end_exclusive(self):
        value, _ = EVALUATOR.read_json3(json3([timed_event(words=("before", " inside", " after"))]), (0.5, 1))
        self.assertEqual(EVALUATOR.normalize(value), ["inside"])

    def test_vtt_rolling_line_not_spoken_twice(self):
        content = """WEBVTT

00:00:00.000 --> 00:00:02.000
hello<00:00:00.500><c> world</c>

00:00:01.000 --> 00:00:03.000
hello world
hello<00:00:01.500><c> world</c>
"""
        value, evidence = EVALUATOR.read_vtt(content, (0, 4))
        self.assertEqual(EVALUATOR.normalize(value), ["hello", "world", "hello", "world"])
        self.assertEqual(evidence["rolling_lines_removed"], 1)

    def test_plain_vtt_genuine_repetitions_preserved(self):
        content = "WEBVTT\n\n00:00.000 --> 00:01.000\ngo go\n\n00:01.000 --> 00:02.000\ngo go\n"
        value, _ = EVALUATOR.read_vtt(content, (0, 2))
        self.assertEqual(EVALUATOR.normalize(value), ["go"] * 4)

    def test_overlapping_plain_vtt_rejected(self):
        content = "WEBVTT\n\n00:00.000 --> 00:02.000\ngo go\n\n00:01.000 --> 00:03.000\ngo go\n"
        with self.assertRaisesRegex(EVALUATOR.EvidenceError, "overlapping plain"):
            EVALUATOR.read_vtt(content, (0, 5))

    def test_untimed_boundary_crossing_rejected(self):
        content = json.dumps({"type": "cue", "start": 0, "end": 2, "text": "not safely croppable"})
        with self.assertRaisesRegex(EVALUATOR.EvidenceError, "crosses clip"):
            EVALUATOR.read_koe(content, (1, 3), 0)

    def test_koe_extracted_clip_origin(self):
        content = json.dumps({"type": "cue", "start": 0, "end": 2, "text": "hello world"})
        self.assertEqual(EVALUATOR.read_koe(content, (10, 12), 10), "hello world")

    def test_koe_ready_is_not_speech_but_errors_and_gaps_invalidate_capture(self):
        cue = {"type": "cue", "start": 0, "end": 2, "text": "actual speech"}
        stream = "\n".join(json.dumps(event) for event in [{"type": "ready"}, cue])
        self.assertEqual(EVALUATOR.read_koe(stream, (0, 2), 0), "actual speech")
        for event in [{"type": "ready"}, {"type": "error", "message": "failed"},
                      {"type": "gap", "message": "audio dropped"}]:
            with self.assertRaises(EVALUATOR.EvidenceError):
                EVALUATOR.read_koe(stream + "\n" + json.dumps(event), (0, 2), 0)

    def test_koe_adjacent_excluded_cues_do_not_leak(self):
        content = "\n".join(json.dumps({"type": "cue", "start": start, "end": start + 1, "text": value})
                            for start, value in enumerate(("before", "inside", "after")))
        self.assertEqual(EVALUATOR.read_koe(content, (1, 2), 0), "inside")


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)

    def video(self, identifier="fixture", reference="hello world", youtube="hello earth", koe="hello world"):
        for suffix, content in (("reference.txt", reference),
                                ("auto.json3", json3([timed_event(words=tuple(youtube.split()))])),
                                ("koe.ndjson", json.dumps({"type": "cue", "start": 0, "end": 2, "text": koe}))):
            (self.base / f"{identifier}.{suffix}").write_text(content, encoding="utf-8")
        return {"id": identifier, "source_url": f"https://www.youtube.com/watch?v={identifier}",
                "language": "en", "clip": {"start": 0, "end": 5},
                "reference": {"path": f"{identifier}.reference.txt", "video_id": identifier,
                    "language": "en", "clip": {"start": 0, "end": 5}, "validated": True,
                    "source_url": "fixture://invented-test-text", "provenance": "Synthetic parser fixture, not real evaluation",
                    "validation": {"basis": "audio_review", "reviewer": "fixture",
                        "method": "Synthetic test declaration only", "reviewed_at": "2026-11-05"}},
                "youtube_auto": {"path": f"{identifier}.auto.json3", "format": "json3", "language": "en",
                    "provenance": {"kind": "youtube_auto", "video_id": identifier,
                        "source_url": f"https://www.youtube.com/watch?v={identifier}", "retrieved_at": "2026-11-05"}},
                "koe": {"path": f"{identifier}.koe.ndjson"}}

    def report(self, *videos):
        return EVALUATOR.evaluate_manifest({"schema_version": 1, "videos": list(videos)}, self.base)

    def test_valid_win_retains_audit_evidence(self):
        report = self.report(self.video())
        self.assertTrue(report["gate"]["passed"])
        item = report["videos"][0]
        self.assertEqual(item["scores"]["youtube_auto"]["substitutions"], 1)
        self.assertEqual(len(item["evidence"]["reference"]["sha256"]), 64)
        self.assertEqual(item["normalization"]["reference"], ["hello", "world"])

    def test_missing_unvalidated_and_mismatched_references_rejected(self):
        for field, value in (("validated", False), ("validated", "true"), ("video_id", "other"),
                             ("language", "fr"), ("clip", {"start": 0, "end": 4}),
                             ("validation", {}), ("path", "missing.txt")):
            with self.subTest(field=field, value=value):
                video = self.video()
                video["reference"][field] = value
                report = self.report(video)
                self.assertFalse(report["complete"])
                self.assertFalse(report["gate"]["passed"])
                self.assertEqual(report["counts"]["invalid"], 1)

    def test_missing_auto_provenance_and_authored_captions_rejected(self):
        for provenance in (None, {}, {"kind": "youtube_authored"}):
            with self.subTest(provenance=provenance):
                video = self.video()
                video["youtube_auto"]["provenance"] = provenance
                self.assertFalse(self.report(video)["complete"])

    def test_empty_auto_caption_baseline_is_incomplete(self):
        video = self.video()
        (self.base / video["youtube_auto"]["path"]).write_text(json3([]), encoding="utf-8")
        self.assertFalse(self.report(video)["complete"])

    def test_reference_review_hash_mismatch(self):
        video = self.video()
        video["reference"]["validation"]["sha256"] = "0" * 64
        self.assertIn("SHA-256 mismatch", self.report(video)["videos"][0]["error"])

    def test_regression_not_hidden_by_micro_win(self):
        win = self.video("win", "one two three", "four five six", "one two three")
        loss = self.video("loss", "one", "one", "wrong")
        tie = self.video("tie", "one", "one", "one")
        report = self.report(win, loss, tie)
        aggregate = report["micro_aggregate"]["scores"]
        self.assertLess(aggregate["koe"]["wer"], aggregate["youtube_auto"]["wer"])
        self.assertEqual(aggregate["koe"]["wer"], 1 / 5)
        self.assertEqual(report["gate"]["regressions"], ["loss"])
        self.assertEqual(report["gate"]["ties"], ["tie"])
        self.assertFalse(report["gate"]["passed"])
        self.assertEqual(len(report["videos"]), 3)

    def test_invalid_video_cannot_be_omitted_from_gate(self):
        good, bad = self.video("good"), self.video("bad")
        bad["reference"]["validated"] = False
        report = self.report(good, bad)
        self.assertEqual(report["counts"]["evaluated"], 1)
        self.assertFalse(report["gate"]["passed"])
        self.assertEqual(report["micro_aggregate"]["scope"], "valid_videos_only_incomplete")

    def test_publisher_reference_scores_are_provisional_even_when_koe_wins(self):
        video = self.video()
        video["reference"]["validated"] = False
        video["reference"]["validation"]["basis"] = "publisher_human_transcript"
        video["reference"]["validation"]["method"] = "Publisher attribution checked; no independent audio review"
        report = self.report(video)
        self.assertEqual(report["videos"][0]["outcome"], "win")
        self.assertEqual(report["counts"]["evaluated"], 1)
        self.assertEqual(report["counts"]["provisional"], 1)
        self.assertEqual(report["counts"]["validated"], 0)
        self.assertFalse(report["videos"][0]["reference_validated"])
        self.assertTrue(report["videos"][0]["provisional"])
        self.assertFalse(report["complete"])
        self.assertFalse(report["gate"]["passed"])
        self.assertEqual(report["gate"]["unvalidated_references"], ["fixture"])
        self.assertTrue(report["micro_aggregate"]["provisional"])

    def test_publisher_reference_cannot_claim_validated_true(self):
        video = self.video()
        video["reference"]["validation"]["basis"] = "publisher_human_transcript"
        self.assertEqual(self.report(video)["counts"]["invalid"], 1)

    def test_validation_basis_cannot_be_inferred_from_prose(self):
        video = self.video()
        del video["reference"]["validation"]["basis"]
        video["reference"]["validation"]["method"] = "audio_review"
        self.assertEqual(self.report(video)["counts"]["invalid"], 1)

    def test_duplicate_video_ids_invalid(self):
        video = self.video()
        report = self.report(video, copy.deepcopy(video))
        self.assertEqual(report["counts"]["invalid"], 2)


if __name__ == "__main__":
    unittest.main()
