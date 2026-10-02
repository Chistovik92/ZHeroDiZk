#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Fetch pinned upstream client source into a NEW directory; never update an existing tree."""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def validate_lock(lock):
    client = lock["client"]
    if lock.get("schema_version") != 1:
        raise ValueError("Unsupported lock schema")
    if client["url"] != "https://github.com/rustdesk/rustdesk.git":
        raise ValueError("Unexpected upstream URL")
    if not re.fullmatch(r"[0-9a-f]{40}", client["commit"]):
        raise ValueError("Expected full immutable commit SHA")
    return client


def validate_server_lock(lock):
    server = lock["server"]
    if lock.get("schema_version") != 1:
        raise ValueError("Unsupported lock schema")
    if server["url"] != "https://github.com/rustdesk/rustdesk-server.git":
        raise ValueError("Unexpected upstream URL")
    if not re.fullmatch(r"[0-9a-f]{40}", server["commit"]):
        raise ValueError("Expected full immutable commit SHA")
    return server


def git(destination, *args):
    return subprocess.run(["git", "-C", str(destination), *args], check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          text=True, timeout=180).stdout.strip()


def checkout(client, destination, submodules=False):
    destination = Path(destination).resolve()
    # Exclusive creation avoids overwriting user changes, including empty dirs.
    destination.mkdir(parents=True, exist_ok=False)
    git(destination, "init")
    git(destination, "remote", "add", "origin", client["url"])
    git(destination, "fetch", "--depth=1", "origin", client["commit"])
    git(destination, "checkout", "--detach", "FETCH_HEAD")
    actual = git(destination, "rev-parse", "HEAD")
    if actual != client["commit"]:
        raise RuntimeError("Upstream commit mismatch")
    if not (destination / client.get("license_file", "LICENCE")).is_file():
        raise RuntimeError("Upstream license missing")
    if submodules:
        git(destination, "submodule", "update", "--init", "--recursive")
    return actual


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", choices=("client", "server"), default="client")
    parser.add_argument("--destination", type=Path, default=None)
    parser.add_argument("--submodules", action="store_true")
    parser.add_argument("--check-lock", action="store_true")
    args = parser.parse_args(argv)
    try:
        lock = json.loads((ROOT / "upstream.lock.json").read_text(encoding="utf-8"))
        client = validate_lock(lock)
        server = validate_server_lock(lock) if "server" in lock else None
        if args.check_lock:
            print(json.dumps({"client": client, "server": server}, indent=2))
            return 0
        if args.component == "server" and server is None:
            raise ValueError("No server entry in upstream.lock.json")
        component = client if args.component == "client" else server
        destination = args.destination or ROOT / "vendor" / args.component
        sha = checkout(component, destination, args.submodules)
        print(f"Checked out {sha} into {destination}")
        if not args.submodules:
            print("Submodules not downloaded; build requires --submodules in a new destination.")
        print("This is unmodified upstream source, not a branded client build.")
        return 0
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        if isinstance(exc, subprocess.CalledProcessError):
            print(exc.stderr, file=sys.stderr)
        print("No existing checkout was overwritten. A new partial directory may remain; inspect it before removal.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
