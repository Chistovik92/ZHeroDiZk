#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Prepare a clean, pinned upstream checkout for the Linux baseline build."""
import argparse
import json
import subprocess
from pathlib import Path

if __package__:
    from .bootstrap import ROOT, git, validate_lock
else:
    from bootstrap import ROOT, git, validate_lock


def prepare(source):
    source = source.resolve()
    client = validate_lock(json.loads((ROOT / "upstream.lock.json").read_text()))
    if git(source, "rev-parse", "HEAD") != client["commit"]:
        raise ValueError("Source does not match upstream.lock.json")
    if git(source, "status", "--porcelain"):
        raise ValueError("Source must be clean; refusing to overwrite changes")
    submodules = git(source, "submodule", "status", "--recursive")
    if not submodules or any(line.startswith(("-", "+", "U")) for line in submodules.splitlines()):
        raise ValueError("Pinned submodules must be initialized without changes")
    cargo = source / "Cargo.toml"
    original = cargo.read_text()
    needle = '["cdylib", "staticlib", "rlib"]'
    if original.count(needle) != 1:
        raise ValueError("Upstream Cargo layout changed; review the build adjustment")
    cargo.write_text(original.replace(needle, '["cdylib"]'))
    return {"upstream": client, "submodules": submodules,
            "adjustments": ["Build cdylib only, matching upstream Linux CI"],
            "branding": "Upstream UI retained for baseline verification",
            "policy_integrated": False, "distribution": "build verification only"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    try:
        report = prepare(args.source)
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f"ERROR: {exc}")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
