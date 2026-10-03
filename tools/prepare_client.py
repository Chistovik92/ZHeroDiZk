#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Prepare a clean, pinned upstream checkout for the Linux baseline build."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

if __package__:
    from .bootstrap import ROOT, git, validate_lock
else:
    from bootstrap import ROOT, git, validate_lock


def read_source(path):
    # newline="" keeps upstream line endings byte-for-byte on every OS.
    with open(path, encoding="utf-8", newline="") as handle:
        return handle.read()


def write_source(path, text):
    with open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write(text)


def collect_changes(source, manifest, pending):
    """Validate every manifest change against the files and queue the new text."""
    source = Path(source).resolve()
    for change in manifest["changes"]:
        path = (source / change["path"]).resolve()
        if not path.is_relative_to(source) or path == source:
            raise ValueError("Identity change escapes source checkout")
        content = pending.get(path)
        if content is None:
            content = read_source(path)
        if content.count(change["before"]) != 1:
            raise ValueError(f"Upstream identity layout changed: {change['path']}")
        pending[path] = content.replace(change["before"], change["after"])


def collect_files(source, manifest, pending_files):
    """Queue whole-file replacements (icons). The original must be the expected upstream file."""
    source = Path(source).resolve()
    for item in manifest.get("files", []):
        path = (source / item["path"]).resolve()
        if not path.is_relative_to(source) or path == source:
            raise ValueError("File replacement escapes source checkout")
        try:
            current = path.read_bytes()
        except OSError as exc:
            raise ValueError(f"Upstream file missing: {item['path']}") from exc
        if item.get("normalize_eol"):
            current = current.replace(bytes([13, 10]), bytes([10]))
        if hashlib.sha256(current).hexdigest() != item["sha256_before"]:
            raise ValueError(f"Upstream file changed: {item['path']}")
        pending_files[path] = (ROOT / item["source"]).read_bytes()


def write_pending(pending, pending_files=None):
    # Every expected source fragment is validated before anything is written.
    # If a write still fails, restore the files already changed.
    pending_files = pending_files or {}
    originals = {}
    original_files = {}
    try:
        for path, content in pending.items():
            originals[path] = read_source(path)
            write_source(path, content)
        for path, data in pending_files.items():
            original_files[path] = path.read_bytes()
            path.write_bytes(data)
    except OSError:
        for path, text in originals.items():
            write_source(path, text)
        for path, data in original_files.items():
            path.write_bytes(data)
        raise


def prepare(source, profile="baseline", platform="linux"):
    if profile not in ("baseline", "zherodizk"):
        raise ValueError("Unknown client profile")
    if platform not in ("linux", "windows"):
        raise ValueError("Unknown client platform")
    source = source.resolve()
    client = validate_lock(json.loads((ROOT / "upstream.lock.json").read_text(encoding="utf-8")))
    if git(source, "rev-parse", "HEAD") != client["commit"]:
        raise ValueError("Source does not match upstream.lock.json")
    if git(source, "status", "--porcelain"):
        raise ValueError("Source must be clean; refusing to overwrite changes")
    submodules = git(source, "submodule", "status", "--recursive")
    if not submodules or any(line.startswith(("-", "+", "U")) for line in submodules.splitlines()):
        raise ValueError("Pinned submodules must be initialized without changes")
    pending = {}
    pending_files = {}
    adjustments = []
    if platform == "linux":
        cargo = source / "Cargo.toml"
        original = read_source(cargo)
        needle = '["cdylib", "staticlib", "rlib"]'
        if original.count(needle) != 1:
            raise ValueError("Upstream Cargo layout changed; review the build adjustment")
        pending[cargo] = original.replace(needle, '["cdylib"]')
        adjustments.append("Build cdylib only, matching upstream Linux CI")
    identity = None
    if profile == "zherodizk":
        # The common manifest removes built-in third-party servers, keys and update checks.
        for name in ("common", platform):
            manifest = json.loads((ROOT / "client" / f"{name}-identity.json").read_text(encoding="utf-8"))
            if manifest.get("schema_version") != 1 or manifest.get("profile") != profile:
                raise ValueError("Unsupported identity manifest")
            collect_changes(source, manifest, pending)
            collect_files(source, manifest, pending_files)
            if name == platform:
                identity = manifest
    write_pending(pending, pending_files)
    return {"upstream": client, "submodules": submodules,
            "adjustments": adjustments, "platform": platform,
            "profile": profile,
            "identity": {k: identity[k] for k in ("display_name", "binary_name", "gtk_application_id") if k in identity} if identity else None,
            "changed_files": [p.relative_to(source).as_posix() for p in [*pending, *pending_files]],
            "branding": f"{platform.capitalize()} identity changed; upstream artwork retained" if identity else "Upstream UI retained for baseline verification",
            "policy_integrated": False, "distribution": "build verification only"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--profile", choices=("baseline", "zherodizk"), default="baseline")
    parser.add_argument("--platform", choices=("linux", "windows"), default="linux")
    args = parser.parse_args()
    try:
        report = prepare(args.source, args.profile, args.platform)
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f"ERROR: {exc}")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
