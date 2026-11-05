#!/usr/bin/env python3
"""Compare reviewed references with YouTube auto captions and Koe cue NDJSON.

Usage: evaluate-captions.py MANIFEST --output REPORT.json
JSON goes to stdout and optionally REPORT.json; the per-video table goes to
stderr. Exit 0 requires a strict Koe WER win on EVERY manifest video; exit 1
means at least one tie/loss; exit 2 means invalid/incomplete/provisional evidence.
Invalid videos remain reported, and provisional comparisons never pass the gate.

Manifest schema (paths relative to the manifest, times in source seconds):
  {"schema_version": 1, "videos": [{
    "id": "VIDEO_ID", "source_url": "https://www.youtube.com/watch?v=VIDEO_ID",
    "language": "en", "clip": {"start": 0, "end": 60},
    "reference": {"path": "reviewed.txt", "video_id": "VIDEO_ID",
      "language": "en", "clip": {"start": 0, "end": 60},
      "source_url": "https://example.org/transcript", "provenance": "...",
      "validated": true, "validation": {"basis": "audio_review", "reviewer": "...",
        "method": "Reviewed against the exact source audio clip",
        "reviewed_at": "2026-11-05"}},
    "youtube_auto": {"path": "auto.json3", "format": "json3", "language": "en",
      "provenance": {"kind": "youtube_auto", "video_id": "VIDEO_ID",
        "source_url": "https://www.youtube.com/watch?v=VIDEO_ID",
        "retrieved_at": "2026-11-05"}},
    "koe": {"path": "koe.ndjson", "time_origin": 0}
  }]}

validated:true with validation.basis:"audio_review" is the reviewer's explicit
attestation of an independent, verbatim reference reviewed against precisely
this video/language/clip. validation.basis:"publisher_human_transcript" requires
validated:false and permits PROVISIONAL numeric comparisons only, never a
validated accuracy claim or gate pass. Unreviewed references without that
explicit publisher provenance basis are rejected. Validation metadata is
preserved, not independently certified by this tool; model-generated truth
or either hypothesis must not be declared an independent reference.
Optional reference.validation.sha256 pins the reviewed UTF-8 file's bytes.

Koe timestamps are seconds, offset by time_origin (use clip.start for an
extracted clip). YouTube times always use original source coordinates.
Whole cues crossing clip boundaries are rejected, not silently cropped. Timed
YouTube words are selected by onset in [start,end). Plain overlapping VTT is
unsupported: use original JSON3 or timestamped WebVTT. JSON3 supports timed
segments, nonoverlapping untimed cues, and explicit append events. Ambiguous
replacements/corrections are rejected rather than guessed or text-deduplicated.
"""
import argparse
from datetime import date, datetime
import hashlib
import html
import json
import math
from pathlib import Path
import re
import sys
import unicodedata
from urllib.parse import parse_qs, urlparse


class EvidenceError(ValueError):
    """Evidence cannot support a defensible comparison."""


def require(condition, message):
    if not condition:
        raise EvidenceError(message)


def mapping(value, name):
    require(isinstance(value, dict), f"{name} must be an object")
    return value


def text(value, name):
    require(isinstance(value, str) and bool(value.strip()), f"{name} must be nonempty text")
    return value


def number(value, name):
    require(isinstance(value, (int, float)) and not isinstance(value, bool)
            and math.isfinite(value) and value >= 0, f"{name} must be finite and nonnegative")
    return value


def timestamp(value, name):
    value = text(value, name)
    try:
        if "T" in value:
            datetime.fromisoformat(value.replace("Z", "+00:00"))
        else:
            date.fromisoformat(value)
    except ValueError as error:
        raise EvidenceError(f"{name} must be an ISO date or datetime") from error


def clip_range(value, name="clip"):
    value = mapping(value, name)
    start = number(value.get("start"), f"{name}.start")
    end = number(value.get("end"), f"{name}.end")
    require(end > start, f"{name}.end must exceed start")
    return start, end


def youtube_id(url):
    parsed = urlparse(text(url, "YouTube source_url"))
    require(parsed.scheme == "https", "YouTube source_url must use HTTPS")
    if parsed.hostname == "youtu.be":
        return parsed.path.strip("/")
    require(parsed.hostname in {"youtube.com", "www.youtube.com", "m.youtube.com"},
            "source_url is not a YouTube URL")
    if parsed.path == "/watch":
        return parse_qs(parsed.query).get("v", [None])[0]
    match = re.fullmatch(r"/(?:shorts|live|embed)/([^/]+)/?", parsed.path)
    return match.group(1) if match else None


def normalize(value):
    """NFKC + casefold; letters/numbers/marks; internal apostrophes retained.

    Punctuation and symbols become boundaries, except straight/curly internal
    apostrophes. No stopword removal, number expansion, stemming, annotation
    removal, or repeated-word removal. Whitespace-delimited WER, not CJK CER.
    """
    value = unicodedata.normalize("NFKC", value).casefold().replace("’", "'")
    chars = []
    for index, char in enumerate(value):
        if unicodedata.category(char)[0] in "LNM":
            chars.append(char)
        elif (char == "'" and index > 0 and index + 1 < len(value)
              and value[index - 1].isalnum() and value[index + 1].isalnum()):
            chars.append(char)
        else:
            chars.append(" ")
    return "".join(chars).split()


def wer(reference, hypothesis):
    """Linear-space Levenshtein counts; tie order match/substitute/delete/insert."""
    previous = [(index, 0, 0, index) for index in range(len(hypothesis) + 1)]
    for row, expected in enumerate(reference, 1):
        current = [(row, 0, row, 0)]
        for column, actual in enumerate(hypothesis, 1):
            cost, substitutions, deletions, insertions = previous[column - 1]
            changed = int(expected != actual)
            diagonal = (cost + changed, substitutions + changed, deletions, insertions)
            cost, substitutions, deletions, insertions = previous[column]
            delete = (cost + 1, substitutions, deletions + 1, insertions)
            cost, substitutions, deletions, insertions = current[column - 1]
            insert = (cost + 1, substitutions, deletions, insertions + 1)
            current.append(min((diagonal, delete, insert), key=lambda item: item[0]))
        previous = current
    errors, substitutions, deletions, insertions = previous[-1]
    return {"reference_words": len(reference), "hypothesis_words": len(hypothesis),
            "substitutions": substitutions, "deletions": deletions,
            "insertions": insertions, "errors": errors,
            "wer": errors / len(reference) if reference else None}


def select_cue(start, end, value, clip):
    require(end > start, "caption end must exceed start")
    if end <= clip[0] or start >= clip[1]:
        return ""
    require(start >= clip[0] and end <= clip[1],
            f"untimed cue [{start},{end}) crosses clip {clip}; align clip or supply word times")
    return value


class TimedWords:
    def __init__(self, clip):
        self.clip = clip
        self.seen = {}
        self.words = []
        self.deduplicated = 0

    def add(self, start, value, window=0):
        if not self.clip[0] <= start < self.clip[1]:
            return
        # Replay is proven by identical source time AND window AND text. Never
        # remove a suffix merely because it repeats a previously spoken phrase.
        key = (window, start)
        signature = tuple(normalize(value))
        if not signature:
            return
        if key in self.seen:
            require(self.seen[key] == signature,
                    f"conflicting caption revisions at source time {start}; unsupported")
            self.deduplicated += 1
            return
        self.seen[key] = signature
        self.words.append((start, len(self.words), value))

    def result(self):
        return " ".join(value for _, _, value in sorted(self.words))


def read_json3(content, clip):
    payload = mapping(json.loads(content), "JSON3")
    events = payload.get("events")
    require(isinstance(events, list), "JSON3.events must be an array")
    words = TimedWords(clip)
    previous_start = -1
    previous_untimed_end = 0
    timed_end = 0
    for event in events:
        event = mapping(event, "JSON3 event")
        if "segs" not in event:
            # Window layout/clear events contain no spoken text.
            continue
        segments = event["segs"]
        require(isinstance(segments, list), "JSON3 segs must be an array")
        for segment in segments:
            mapping(segment, "JSON3 segment")
            require(isinstance(segment.get("utf8"), str), "JSON3 segment lacks utf8 text")
        segments = [segment for segment in segments if segment["utf8"].strip()]
        if not segments:
            continue
        start_ms = number(event.get("tStartMs"), "JSON3 tStartMs")
        duration_ms = number(event.get("dDurationMs"), "JSON3 dDurationMs")
        require(duration_ms > 0, "JSON3 duration must be positive")
        start, end = start_ms / 1000, (start_ms + duration_ms) / 1000
        require(start >= previous_start, "JSON3 events must be in source time order")
        previous_start = start
        # A full-video caption track may contain unsupported display updates
        # elsewhere. They cannot affect the selected audio interval.
        if end <= clip[0] or start >= clip[1]:
            continue
        window = event.get("wWinId", 0)
        require(isinstance(window, int) and not isinstance(window, bool), "invalid JSON3 window ID")
        append = event.get("aAppend", 0)
        require(type(append) is int and append in (0, 1), "unsupported JSON3 append semantics")
        # Native ASR omits the first word's zero offset, including when a
        # display event contains just that word. Display duration can overlap
        # later words; it is not a second spoken occurrence.
        singleton_asr = (len(segments) == 1 and "acAsrConf" in segments[0]
                         and len(segments[0]["utf8"].split()) == 1)
        word_timed = singleton_asr or any("tOffsetMs" in segment for segment in segments)
        if word_timed:
            require(start >= previous_untimed_end,
                    "timed JSON3 overlaps an untimed display; ambiguous replay")
            last_offset = -1
            for index, segment in enumerate(segments):
                require(index == 0 or "tOffsetMs" in segment,
                        "JSON3 has untimed text after a timed segment")
                offset = number(segment.get("tOffsetMs", 0), "JSON3 tOffsetMs")
                require(last_offset < offset < duration_ms,
                        "JSON3 word offsets must increase and precede cue end")
                last_offset = offset
                words.add((start_ms + offset) / 1000, segment["utf8"], window)
            timed_end = max(timed_end, end)
        else:
            require(append == 1 or start >= max(previous_untimed_end, timed_end),
                    "overlapping untimed JSON3 replacement is unsupported; obtain timed JSON3")
            value = select_cue(start, end, "".join(segment["utf8"] for segment in segments), clip)
            if value:
                words.add(start, value, window)
            previous_untimed_end = max(previous_untimed_end, end)
    return words.result(), {"format": "json3", "timed_replays_removed": words.deduplicated}


VTT_TIME = r"(?:\d{2,}:)?\d{2}:\d{2}\.\d{3}"
INLINE_TIME = re.compile(r"<(" + VTT_TIME + r")>")


def vtt_time(value):
    parts = value.split(":")
    require(len(parts) in (2, 3), "invalid WebVTT timestamp")
    seconds = float(parts[-1])
    minutes = int(parts[-2])
    hours = int(parts[0]) if len(parts) == 3 else 0
    require(seconds < 60 and minutes < 60, "invalid WebVTT timestamp range")
    return hours * 3600 + minutes * 60 + seconds


def vtt_text(value):
    # Only standard presentation tags; don't silently remove unknown markup.
    value = re.sub(r"</?(?:c(?:\.[\w-]+)*|b|i|u|ruby|rt)>", "", value)
    value = re.sub(r"<(?:v|lang) [^<>]+>|</(?:v|lang)>", "", value)
    require("<" not in value and ">" not in value, "unsupported WebVTT markup")
    return html.unescape(value)


def read_vtt(content, clip):
    content = content.lstrip("\ufeff").replace("\r\n", "\n")
    require(content.splitlines() and content.splitlines()[0].split(" ")[0] == "WEBVTT",
            "missing WebVTT header")
    words = TimedWords(clip)
    prior_end = 0
    prior_start = -1
    prior_timed_lines = set()
    rolling_lines_removed = 0
    for block in re.split(r"\n[ \t]*\n", content)[1:]:
        lines = block.strip().splitlines()
        if not lines or lines[0] == "NOTE" or lines[0].startswith("NOTE "):
            continue
        if "-->" not in lines[0] and len(lines) > 1:
            lines = lines[1:]
        require(lines and "-->" in lines[0], "unsupported WebVTT block")
        match = re.fullmatch(r"(" + VTT_TIME + r")\s+-->\s+(" + VTT_TIME + r")(?:\s+.*)?", lines[0])
        require(match is not None, "invalid WebVTT cue timing")
        start, end = map(vtt_time, match.groups())
        require(end > start and start >= prior_start, "invalid or out-of-order WebVTT cue")
        overlap = start < prior_end
        body = lines[1:]
        timed = any(INLINE_TIME.search(line) for line in body)
        timed_lines = set()
        if not timed:
            require(not overlap, "overlapping plain WebVTT is ambiguous; use timed JSON3")
            value = select_cue(start, end, vtt_text(" ".join(body)), clip)
            if value:
                words.add(start, value)
        else:
            fresh_line = False
            for line in body:
                parts = INLINE_TIME.split(line)
                if len(parts) == 1:
                    # YouTube rolling VTT keeps a previously timed display line
                    # above a NEW timestamped line. Only that proven carry-over
                    # is removable; unanchored overlapping text is not guessed.
                    plain = vtt_text(line)
                    require(overlap and not fresh_line and plain in prior_timed_lines,
                            "untimed line in timestamped WebVTT is not a proven rolling carry-over")
                    timed_lines.add(plain)
                    rolling_lines_removed += 1
                    continue
                fresh_line = True
                onset = start
                words.add(onset, vtt_text(parts[0]))
                for index in range(1, len(parts), 2):
                    next_onset = vtt_time(parts[index])
                    require(onset < next_onset < end, "WebVTT word times must increase within cue")
                    onset = next_onset
                    words.add(onset, vtt_text(parts[index + 1]))
                timed_lines.add(vtt_text(INLINE_TIME.sub("", line)))
        prior_start, prior_end, prior_timed_lines = start, end, timed_lines
    return words.result(), {"format": "vtt", "timed_replays_removed": words.deduplicated,
                            "rolling_lines_removed": rolling_lines_removed}


def read_koe(content, clip, origin):
    result = []
    last_start = -1
    ready = False
    for line_number, line in enumerate(content.splitlines(), 1):
        if not line.strip():
            continue
        cue = mapping(json.loads(line), f"Koe line {line_number}")
        if cue.get("type") == "ready":
            require(not ready and last_start < 0, "Koe ready event must occur once before cues")
            ready = True
            continue
        require(cue.get("type") == "cue", f"Koe line {line_number} is not a cue")
        start = number(cue.get("start"), "Koe cue.start") + origin
        end = number(cue.get("end"), "Koe cue.end") + origin
        require(start >= last_start, "Koe cues must be in source time order")
        last_start = start
        require(isinstance(cue.get("text"), str), "Koe cue.text must be text")
        value = select_cue(start, end, cue["text"], clip)
        if value:
            result.append(value)
    return " ".join(result)


def load_input(base, spec, name):
    path = (base / text(spec.get("path"), name + ".path")).resolve()
    raw = path.read_bytes()
    return raw.decode("utf-8-sig"), {"path": str(path), "sha256": hashlib.sha256(raw).hexdigest(),
                                   "bytes": len(raw)}


def evaluate_video(video, base):
    video = mapping(video, "video")
    identifier = text(video.get("id"), "video.id")
    require(youtube_id(video.get("source_url")) == identifier, "video source_url ID mismatch")
    language = text(video.get("language"), "video.language")
    clip = clip_range(video.get("clip"))
    reference = mapping(video.get("reference"), "reference")
    require(reference.get("video_id") == identifier, "reference video_id mismatch")
    require(reference.get("language") == language, "reference language mismatch")
    require(clip_range(reference.get("clip"), "reference.clip") == clip, "reference clip mismatch")
    text(reference.get("source_url"), "reference.source_url")
    text(reference.get("provenance"), "reference.provenance")
    validation = mapping(reference.get("validation"), "reference.validation")
    basis = validation.get("basis")
    require(basis in ("audio_review", "publisher_human_transcript"),
            "reference.validation.basis must be audio_review or publisher_human_transcript")
    reference_validated = basis == "audio_review"
    require(reference.get("validated") is reference_validated,
            "audio_review requires validated:true; publisher_human_transcript requires validated:false")
    text(validation.get("reviewer"), "reference.validation.reviewer")
    text(validation.get("method"), "reference.validation.method")
    timestamp(validation.get("reviewed_at"), "reference.validation.reviewed_at")
    youtube = mapping(video.get("youtube_auto"), "youtube_auto")
    require(youtube.get("language") == language, "auto-caption language mismatch")
    require(youtube.get("time_origin", 0) == 0, "YouTube captions must use source timestamps")
    provenance = mapping(youtube.get("provenance"), "youtube_auto.provenance")
    require(provenance.get("kind") == "youtube_auto", "missing genuine auto-caption provenance")
    require(provenance.get("video_id") == identifier, "auto-caption video_id mismatch")
    require(youtube_id(provenance.get("source_url")) == identifier, "auto-caption source_url ID mismatch")
    timestamp(provenance.get("retrieved_at"), "youtube_auto.provenance.retrieved_at")
    koe = mapping(video.get("koe"), "koe")
    origin = number(koe.get("time_origin", 0), "koe.time_origin")
    reference_text, reference_file = load_input(base, reference, "reference")
    youtube_content, youtube_file = load_input(base, youtube, "youtube_auto")
    koe_content, koe_file = load_input(base, koe, "koe")
    require(len({item["path"] for item in (reference_file, youtube_file, koe_file)}) == 3,
            "reference and hypotheses must be distinct files")
    if "sha256" in validation:
        require(validation["sha256"] == reference_file["sha256"], "reviewed reference SHA-256 mismatch")
    reference_words = normalize(reference_text)
    require(bool(reference_words), "reference contains no normalized words")
    format_name = youtube.get("format")
    require(format_name in ("json3", "vtt"), "youtube_auto.format must be json3 or vtt")
    youtube_text, parsing = (read_json3 if format_name == "json3" else read_vtt)(youtube_content, clip)
    koe_text = read_koe(koe_content, clip, origin)
    youtube_words, koe_words = normalize(youtube_text), normalize(koe_text)
    require(bool(youtube_words), "no auto-caption words in reference clip; incomplete baseline")
    scores = {"youtube_auto": wer(reference_words, youtube_words), "koe": wer(reference_words, koe_words)}
    delta = scores["koe"]["errors"] - scores["youtube_auto"]["errors"]
    return {"id": identifier, "status": "evaluated", "source_url": video["source_url"],
            "language": language, "clip": video["clip"],
            "reference_validated": reference_validated,
            "provisional": not reference_validated,
            "limitations": [] if reference_validated else [
                "Publisher-human transcript only: independent audio review and exact clip alignment unverified; non-gating comparison"],
            "outcome": "win" if delta < 0 else "loss" if delta > 0 else "tie",
            "koe_minus_youtube_errors": delta,
            "koe_minus_youtube_wer": delta / len(reference_words),
            "scores": scores, "normalization": {"reference": reference_words,
                "youtube_auto": youtube_words, "koe": koe_words},
            "evidence": {"reference": reference_file, "youtube_auto": youtube_file, "koe": koe_file,
                "reference_source_url": reference["source_url"],
                "reference_provenance": reference["provenance"], "validation": validation,
                "youtube_auto_provenance": provenance, "youtube_parsing": parsing,
                "koe_time_origin": origin}}


def evaluate_manifest(manifest, base):
    manifest = mapping(manifest, "manifest")
    require(type(manifest.get("schema_version")) is int and manifest["schema_version"] == 1,
            "schema_version must be 1")
    videos = manifest.get("videos")
    require(isinstance(videos, list) and videos, "videos must be a nonempty array")
    identifiers = [item.get("id") if isinstance(item, dict) else None for item in videos]
    duplicate_ids = {identifier for identifier in identifiers if isinstance(identifier, str)
                     and identifiers.count(identifier) > 1}
    results = []
    for index, video in enumerate(videos):
        identifier = identifiers[index]
        try:
            require(isinstance(identifier, str), "video.id must be text")
            require(identifier not in duplicate_ids, "duplicate video.id; each video must appear once")
            results.append(evaluate_video(video, base))
        except (EvidenceError, OSError, UnicodeError, json.JSONDecodeError) as error:
            results.append({"id": identifier, "manifest_index": index,
                            "status": "invalid", "error": str(error)})
    evaluated = [item for item in results if item["status"] == "evaluated"]
    counts = {outcome: sum(item.get("outcome") == outcome for item in results)
              for outcome in ("win", "tie", "loss")}
    counts.update({"expected": len(videos), "evaluated": len(evaluated),
                   "invalid": len(videos) - len(evaluated),
                   "provisional": sum(item["provisional"] for item in evaluated),
                   "validated": sum(item["reference_validated"] for item in evaluated)})
    aggregate = {}
    for system in ("youtube_auto", "koe"):
        totals = {field: sum(item["scores"][system][field] for item in evaluated)
                  for field in ("reference_words", "hypothesis_words", "substitutions", "deletions", "insertions", "errors")}
        totals["wer"] = totals["errors"] / totals["reference_words"] if totals["reference_words"] else None
        aggregate[system] = totals
    complete = counts["invalid"] == 0 and counts["provisional"] == 0
    aggregate_scope = ("all_videos" if complete else
                       "provisional_comparisons_incomplete" if counts["provisional"] else
                       "valid_videos_only_incomplete")
    passed = complete and counts["win"] == len(videos)
    return {"schema_version": 1, "complete": complete,
            "normalization_policy": "Unicode NFKC/casefold; L/N/M words; internal apostrophes retained; other punctuation/symbols become spaces; no word removal",
            "clip_policy": "source seconds [start,end); timed segments by onset; crossing untimed cues rejected",
            "alignment_tie_break": "match/substitution, deletion, insertion",
            "reference_policy": "declared audio review required for gate; publisher-human transcripts provisional only; no generated truth",
            "counts": counts, "videos": results,
            "micro_aggregate": {"scope": aggregate_scope,
                                "provisional": bool(counts["provisional"]), "scores": aggregate},
            "gate": {"rule": "strictly lower Koe WER on every manifest video; ties fail",
                     "passed": passed,
                     "unvalidated_references": [item["id"] for item in evaluated if item["provisional"]],
                     "regressions": [item["id"] for item in results if item.get("outcome") == "loss"],
                     "ties": [item["id"] for item in results if item.get("outcome") == "tie"]}}


def print_table(report):
    print("Video                  Words    YouTube WER     Koe WER   Result", file=sys.stderr)
    for item in report.get("videos", []):
        identifier = str(item["id"])
        if item["status"] != "evaluated":
            print(f"{identifier:22} INVALID: {item['error']}", file=sys.stderr)
            continue
        youtube, koe = item["scores"]["youtube_auto"], item["scores"]["koe"]
        outcome = ("PROVISIONAL " if item["provisional"] else "") + item["outcome"].upper()
        print(f"{identifier:22} {koe['reference_words']:6d} {youtube['wer']:13.2%} {koe['wer']:11.2%}   {outcome}", file=sys.stderr)
        print(f"  S/D/I: YouTube {youtube['substitutions']}/{youtube['deletions']}/{youtube['insertions']}; "
              f"Koe {koe['substitutions']}/{koe['deletions']}/{koe['insertions']}", file=sys.stderr)
        for limitation in item["limitations"]:
            print("  " + limitation, file=sys.stderr)
    if "micro_aggregate" in report:
        aggregate = report["micro_aggregate"]
        print(f"Micro aggregate ({aggregate['scope']}): " + "; ".join(
            f"{name}={score['wer']:.2%}" if score["wer"] is not None else f"{name}=N/A"
            for name, score in aggregate["scores"].items()), file=sys.stderr)
    if "error" in report:
        print("INVALID: " + report["error"], file=sys.stderr)
    print("Strict every-video gate: " + ("PASS" if report["gate"]["passed"] else "FAIL"), file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path, help="also save the JSON report here")
    args = parser.parse_args()
    try:
        raw = args.manifest.read_bytes()
        manifest = json.loads(raw)
        report = evaluate_manifest(manifest, args.manifest.resolve().parent)
        report["manifest"] = {"path": str(args.manifest.resolve()), "sha256": hashlib.sha256(raw).hexdigest()}
    except (EvidenceError, OSError, UnicodeError, json.JSONDecodeError) as error:
        report = {"schema_version": 1, "complete": False, "error": str(error),
                  "videos": [], "gate": {"passed": False}}
    encoded = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
    print_table(report)
    sys.stdout.write(encoded)
    if args.output:
        try:
            require(args.output.resolve() != args.manifest.resolve(), "report must not overwrite manifest")
            for item in report.get("videos", []):
                if item["status"] == "evaluated":
                    for source in ("reference", "youtube_auto", "koe"):
                        require(args.output.resolve() != Path(item["evidence"][source]["path"]),
                                "report must not overwrite evidence")
            args.output.write_text(encoded, encoding="utf-8")
        except (EvidenceError, OSError) as error:
            print(f"Cannot write report: {error}", file=sys.stderr)
            return 2
    return 2 if not report["complete"] else 0 if report["gate"]["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
