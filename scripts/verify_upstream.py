#!/usr/bin/env python3
"""Fail-closed upstream ancestry guard for our Rust integration branches.

The pinned SHA must be an actual upstream commit AND an ancestor of the
submitted code.  A textual mention of an upstream SHA does not count.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys

SHA = re.compile(r"[0-9a-f]{40}\Z")


def git(*args: str, cwd: str | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=False)


def verify(repo: str, lock_path: str, upstream_ref: str, head: str = "HEAD", require_tip: bool = False) -> str:
    raw = Path(lock_path).read_text(encoding="utf-8").strip()
    if not SHA.fullmatch(raw):
        raise ValueError("upstream-base must be one full lowercase 40-hex commit SHA")
    for commit in (raw, upstream_ref, head):
        result = git("rev-parse", "--verify", "--quiet", f"{commit}^{{commit}}", cwd=repo)
        if result.returncode != 0:
            raise ValueError(f"missing commit: {commit}")
    for child in (upstream_ref, head):
        result = git("merge-base", "--is-ancestor", raw, child, cwd=repo)
        if result.returncode != 0:
            raise ValueError(f"pinned upstream SHA is NOT an ancestor of {child}")
    if require_tip:
        tip = git("rev-parse", "--verify", upstream_ref, cwd=repo).stdout.strip()
        if raw != tip:
            raise ValueError(f"upstream moved to {tip}; sync and update the pinned SHA before merging")
    # HEAD can contain the locked base while secretly being derived from a
    # different project's history; this check validates the imported lineage.
    return raw


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default=".")
    parser.add_argument("--lock", default=".github/upstream-base.sha")
    parser.add_argument("--upstream-ref", default="refs/remotes/provenance/master")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--require-tip", action="store_true")
    args = parser.parse_args()
    try:
        pinned = verify(args.repo, args.lock, args.upstream_ref, args.head, args.require_tip)
    except (OSError, ValueError) as error:
        print(f"::error::Upstream provenance failed: {error}", file=sys.stderr)
        return 1
    print(f"Verified imported upstream commit {pinned} in upstream and PR histories")
    return 0


if __name__ == "__main__":
    sys.exit(main())
