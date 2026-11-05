"""Bazel credential-helper protocol; secrets stay out of flags, rc options and BEP."""
import json
import os
from pathlib import Path
import re
import shlex
import sys
from urllib.parse import urlsplit


MARKER = "# sekka-bazel-credential-v1 "
CONFIG_ENV = "SEKKA_BAZEL_CREDENTIAL_CONFIG"


def authority(uri):
    if not isinstance(uri, str):
        raise ValueError("invalid endpoint")
    parsed = urlsplit(uri)
    if parsed.scheme not in ("grpc", "grpcs", "http", "https") or not parsed.hostname or parsed.username or parsed.password:
        raise ValueError("invalid endpoint")
    port = parsed.port
    if port is None:
        port = 80 if parsed.scheme in ("http", "grpc") else 443
    # gRPC uses HTTPS credential metadata, including on plaintext channels.
    # Preserve the endpoint's actual default port before normalizing that scheme.
    return ("http" if parsed.scheme == "http" else "https"), parsed.hostname.lower(), port


def load_config(path):
    """Validate without including private values in any diagnostic."""
    try:
        path = Path(path)
        if path.stat().st_mode & 0o077:
            raise ValueError("private configuration required")
        records = []
        for line in path.read_text().splitlines():
            if line.startswith(MARKER):
                records.append(json.loads(line[len(MARKER):]))
            elif line.strip() and not line.lstrip().startswith("#"):
                words = shlex.split(line)
                if words[0] in ("import", "try-import") or any(
                    word.split("=", 1)[0] in ("--remote_header", "--remote_cache_header", "--remote_exec_header")
                    for word in words
                ):
                    raise ValueError("inline credentials or imports are not supported")
        if len(records) != 1:
            raise ValueError("regenerate client configuration")
        record = records[0]
        authority(record["endpoint"])
        if not isinstance(record["token"], str) or not re.fullmatch(r"[A-Za-z0-9._~+/-]+=*", record["token"]):
            raise ValueError("invalid token")
        return record
    except (OSError, ValueError, TypeError, KeyError, IndexError):
        raise ValueError("Invalid private Bazel credentials; regenerate with build-service client --format bazel") from None


def credentials(config, uri):
    # HTTP downloads must not receive credentials for an HTTPS/gRPC origin,
    # even when they name the same host and explicit port.
    try:
        matches = authority(uri) == authority(config["endpoint"])
    except (ValueError, TypeError, AttributeError):
        matches = False
    return {"headers": {"Authorization": ["Bearer " + config["token"]]}} if matches else {"headers": {}}


def main():
    try:
        if sys.argv[1:] != ["get"]:
            raise ValueError("Expected credential-helper command: get")
        config = load_config(os.environ[CONFIG_ENV])
        request = json.load(sys.stdin)
        response = credentials(config, request["uri"])
    except (ValueError, OSError, KeyError, TypeError):
        print("Unable to load private Bazel credentials or credential-helper request", file=sys.stderr)
        return 1
    json.dump(response, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
