# SPDX-License-Identifier: GPL-3.0-or-later
"""Canonical JSON, byte for byte as sekka-mac-contract's canonical.rs writes it.

- Object keys in the byte order of their UTF-8 encoding (for valid Unicode
  that is code-point order).
- No insignificant whitespace.
- Integers only, within what serde_json reads as an integer: i64 and u64.
- Strings escaped as serde_json escapes them: `"` and `\\`, the short forms
  `\\b \\f \\n \\r \\t`, every other character below U+0020 as `\\u00xx` in
  lower-case hex, and everything else literal UTF-8.

A reader requires canonical input: bytes that aren't the canonical form of
what they parse to are refused, so two byte strings never name one document.
"""

import hashlib
import json

MAX_DEPTH = 32
I64_MIN = -(1 << 63)
U64_MAX = (1 << 64) - 1

_SHORT = {
    '"': '\\"',
    "\\": "\\\\",
    "\b": "\\b",
    "\f": "\\f",
    "\n": "\\n",
    "\r": "\\r",
    "\t": "\\t",
}


class CanonicalError(ValueError):
    """A value has no canonical form, or bytes aren't canonical."""


class TooLarge(CanonicalError):
    pass


class UnsupportedSchema(CanonicalError):
    pass


class NotCanonical(CanonicalError):
    pass


def _string(s, out):
    out.append('"')
    for ch in s:
        code = ord(ch)
        if ch in _SHORT:
            out.append(_SHORT[ch])
        elif code < 0x20:
            out.append("\\u%04x" % code)
        elif 0xD800 <= code <= 0xDFFF:
            raise CanonicalError("strings are valid Unicode; a lone surrogate isn't")
        else:
            out.append(ch)
    out.append('"')


def _write(value, out, depth):
    if depth > MAX_DEPTH:
        raise CanonicalError("document nests too deeply")
    if value is None:
        out.append("null")
    elif value is True:
        out.append("true")
    elif value is False:
        out.append("false")
    elif isinstance(value, int):
        if not I64_MIN <= value <= U64_MAX:
            raise CanonicalError("integers are 64-bit")
        out.append(str(int(value)))
    elif isinstance(value, float):
        raise CanonicalError("documents contain integers only, never fractions")
    elif isinstance(value, str):
        _string(value, out)
    elif isinstance(value, (list, tuple)):
        out.append("[")
        for i, item in enumerate(value):
            if i:
                out.append(",")
            _write(item, out, depth + 1)
        out.append("]")
    elif isinstance(value, dict):
        for key in value:
            if not isinstance(key, str):
                raise CanonicalError("object keys are strings")
        keys = sorted(value, key=lambda k: k.encode("utf-8", "strict"))
        out.append("{")
        for i, key in enumerate(keys):
            if i:
                out.append(",")
            _string(key, out)
            out.append(":")
            _write(value[key], out, depth + 1)
        out.append("}")
    else:
        raise CanonicalError(f"{type(value).__name__} has no JSON form")


def dumps(value):
    """The canonical bytes of a JSON value made of dicts, lists, strings,
    integers, booleans and None."""
    out = []
    _write(value, out, 0)
    try:
        return "".join(out).encode("utf-8", "strict")
    except UnicodeEncodeError as error:
        raise CanonicalError("strings are valid Unicode") from error


def digest_bytes(data):
    """`sha256:` and 64 lower-case hex digits."""
    return "sha256:" + hashlib.sha256(data).hexdigest()


def digest(value):
    """The digest of a value's canonical form."""
    return digest_bytes(dumps(value))


def _reject_constant(name):
    raise CanonicalError(f"{name} is not JSON")


def _reject_float(text):
    raise CanonicalError("documents contain integers only, never fractions")


def parse_json(data):
    """Parse JSON strictly: no NaN or infinities, no fractions."""
    if isinstance(data, (bytes, bytearray)):
        try:
            data = bytes(data).decode("utf-8", "strict")
        except UnicodeDecodeError as error:
            raise CanonicalError("documents are UTF-8") from error
    try:
        return json.loads(data, parse_constant=_reject_constant, parse_float=_reject_float)
    except json.JSONDecodeError as error:
        raise CanonicalError(f"not JSON: {error}") from error


def _check_schema(value, schema):
    found = value.get("schema") if isinstance(value, dict) else None
    if not isinstance(found, str) or found != schema:
        shown = found[:80] if isinstance(found, str) else ""
        raise UnsupportedSchema(f"expected a {schema} document, found {shown!r}")


def loads(data, limit, schema):
    """Parse canonical bytes of at most `limit` bytes whose `schema` is
    `schema`, as canonical::from_slice does: size first, then schema, then
    canonical form."""
    if len(data) > limit:
        raise TooLarge(f"the document is {len(data)} bytes; at most {limit} are accepted")
    value = parse_json(data)
    _check_schema(value, schema)
    if dumps(value) != bytes(data):
        raise NotCanonical("the document isn't in canonical form")
    return value


def loads_relaxed(data, limit, schema):
    """Parse a document whose bytes need not be canonical, still checking
    its size and schema first."""
    if len(data) > limit:
        raise TooLarge(f"the document is {len(data)} bytes; at most {limit} are accepted")
    value = parse_json(data)
    _check_schema(value, schema)
    return value
