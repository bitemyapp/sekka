#!/usr/bin/env python3
"""Select an immutable merge commit for CI, using only base-branch code."""
import json
import os
from pathlib import Path
import re
import time
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen


REPOSITORY = "bitemyapp/sekka"


class NotTrusted(ValueError):
    pass


def api(path):
    request = Request(
        "https://api.github.com/repos/" + REPOSITORY + "/" + path,
        headers={"Authorization": "Bearer " + os.environ["GH_TOKEN"],
                 "Accept": "application/vnd.github+json",
                 "X-GitHub-Api-Version": "2022-11-28"},
    )
    with urlopen(request, timeout=30) as response:
        return json.load(response)


def require_writer(login, get):
    if not login:
        raise NotTrusted("Missing developer identity")
    try:
        permission = get("collaborators/" + quote(login, safe="") + "/permission")
    except HTTPError as error:
        if error.code == 404:
            raise NotTrusted("Developer does not have repository write access") from error
        raise
    # GitHub maps maintain to write and triage to read in this field.
    if permission.get("permission") not in {"admin", "write"}:
        raise NotTrusted("Developer does not have repository write access")


def select_merge(number, actor, triggering_actor, expected_head="", get=api, sleep=time.sleep):
    if not re.fullmatch(r"[1-9][0-9]*", str(number)):
        raise NotTrusted("Invalid pull request number")
    for login in {actor, triggering_actor}:
        require_writer(login, get)
    for attempt in range(6):
        pr = get(f"pulls/{number}")
        if pr["state"] != "open" or pr["draft"]:
            raise NotTrusted("Pull request is closed or a draft")
        if (pr["base"]["ref"] != "main"
                or pr["base"]["repo"]["full_name"] != REPOSITORY
                or (pr["head"].get("repo") or {}).get("full_name") != REPOSITORY):
            raise NotTrusted("Only this repository's branches targeting main are eligible")
        require_writer(pr["user"]["login"], get)
        head, base = pr["head"]["sha"], pr["base"]["sha"]
        if expected_head and head != expected_head:
            raise NotTrusted("Pull request changed since this event; use the newer run")
        merge = pr.get("merge_commit_sha")
        if pr.get("mergeable") is False:
            raise NotTrusted("Pull request has merge conflicts")
        if pr.get("mergeable") and merge:
            if not all(re.fullmatch(r"[0-9a-f]{40}", sha) for sha in [head, base, merge]):
                raise NotTrusted("Invalid commit identity")
            commit = get(f"git/commits/{merge}")
            if [parent["sha"] for parent in commit["parents"]] == [base, head]:
                return {"head": head, "base": base, "merge": merge}
        if attempt < 5:
            sleep(2)
    raise NotTrusted("GitHub has not produced a merge commit for the current head and base")


if __name__ == "__main__":
    if os.environ.get("GITHUB_REPOSITORY") != REPOSITORY:
        raise SystemExit("This gate is only for " + REPOSITORY)
    try:
        selected = select_merge(os.environ["PR_NUMBER"], os.environ["GITHUB_ACTOR"],
                                os.environ["GITHUB_TRIGGERING_ACTOR"],
                                os.environ.get("EXPECTED_HEAD", ""))
    except NotTrusted as error:
        with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
            output.write("allowed=false\n")
        with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a") as summary:
            summary.write(f"PR build not admitted: {error}.\n")
    else:
        with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
            output.write("allowed=true\n")
            for name, sha in selected.items():
                output.write(f"{name}={sha}\n")
        with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a") as summary:
            summary.write(f"Testing head `{selected['head']}` with base `{selected['base']}` "
                          f"at merge `{selected['merge']}`.\n")
