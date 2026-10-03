#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Apply the ZHeroDiZk identity to a clean, pinned server checkout."""
import argparse
import json
import subprocess
from pathlib import Path

if __package__:
    from .bootstrap import ROOT, git, validate_server_lock
    from .prepare_client import collect_changes, write_pending
else:
    from bootstrap import ROOT, git, validate_server_lock
    from prepare_client import collect_changes, write_pending


def prepare(source):
    source = Path(source).resolve()
    server = validate_server_lock(json.loads((ROOT / "upstream.lock.json").read_text(encoding="utf-8")))
    if git(source, "rev-parse", "HEAD") != server["commit"]:
        raise ValueError("Source does not match upstream.lock.json")
    if git(source, "status", "--porcelain"):
        raise ValueError("Source must be clean; refusing to overwrite changes")
    submodules = git(source, "submodule", "status", "--recursive")
    if not submodules or any(line.startswith(("-", "+", "U")) for line in submodules.splitlines()):
        raise ValueError("Pinned submodules must be initialized without changes")
    manifest = json.loads((ROOT / "server" / "identity.json").read_text(encoding="utf-8"))
    if manifest.get("schema_version") != 1 or manifest.get("profile") != "zherodizk":
        raise ValueError("Unsupported identity manifest")
    pending = {}
    collect_changes(source, manifest, pending)
    write_pending(pending)
    return {"upstream": server, "submodules": submodules,
            "binaries": manifest["binaries"],
            "changed_files": [p.relative_to(source).as_posix() for p in pending],
            "distribution": "build verification only"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    try:
        report = prepare(args.source)
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        return 0
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as exc:
        print(f"ERROR: {exc}")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
