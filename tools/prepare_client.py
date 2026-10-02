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


def prepare(source, profile="baseline"):
    if profile not in ("baseline", "zherodizk"):
        raise ValueError("Unknown client profile")
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
    pending = {cargo: original.replace(needle, '["cdylib"]')}
    identity = None
    if profile == "zherodizk":
        identity = json.loads((ROOT / "client/linux-identity.json").read_text())
        if identity.get("schema_version") != 1 or identity.get("profile") != profile:
            raise ValueError("Unsupported identity manifest")
        for change in identity["changes"]:
            path = (source / change["path"]).resolve()
            if not path.is_relative_to(source) or path == source:
                raise ValueError("Identity change escapes source checkout")
            content = pending.get(path)
            if content is None:
                content = path.read_text()
            if content.count(change["before"]) != 1:
                raise ValueError(f"Upstream identity layout changed: {change['path']}")
            pending[path] = content.replace(change["before"], change["after"])
    # Validate every expected source fragment before writing any changes.
    # An I/O failure can still leave a partial checkout; start a new one then.
    for path, content in pending.items():
        path.write_text(content)
    return {"upstream": client, "submodules": submodules,
            "adjustments": ["Build cdylib only, matching upstream Linux CI"],
            "profile": profile,
            "identity": {k: identity[k] for k in ("display_name", "binary_name", "gtk_application_id")} if identity else None,
            "changed_files": [str(p.relative_to(source)) for p in pending],
            "branding": "Linux identity changed; upstream artwork retained" if identity else "Upstream UI retained for baseline verification",
            "policy_integrated": False, "distribution": "build verification only"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--profile", choices=("baseline", "zherodizk"), default="baseline")
    args = parser.parse_args()
    try:
        report = prepare(args.source, args.profile)
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f"ERROR: {exc}")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
