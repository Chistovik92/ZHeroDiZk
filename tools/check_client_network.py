#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Fail if built files still embed a third-party default server, API address or key."""
import sys
from pathlib import Path

FORBIDDEN = (
    b"rs-ny.rustdesk.com",
    b"api.rustdesk.com",
    b"admin.rustdesk.com",
    b"OeVuKk5nlHiXp+APNn0Y3pC1Iwpwn44JGqrQCsWqmBw=",
)
CHUNK = 1 << 20


def scan(path):
    """Return the forbidden strings present in the file (UTF-8 and UTF-16LE forms)."""
    needles = {item: (item, item.decode("ascii").encode("utf-16-le")) for item in FORBIDDEN}
    keep = max(len(n) for forms in needles.values() for n in forms)
    found = set()
    tail = b""
    with open(path, "rb") as handle:
        while True:
            block = handle.read(CHUNK)
            if not block:
                break
            data = tail + block
            for item, forms in needles.items():
                if any(form in data for form in forms):
                    found.add(item.decode("ascii"))
            tail = data[-(keep - 1):]
    return sorted(found)


def main(argv):
    if len(argv) < 2:
        print("usage: check_client_network.py FILE...", file=sys.stderr)
        return 2
    bad = False
    for name in argv[1:]:
        try:
            hits = scan(Path(name))
        except OSError as exc:
            print(f"ERROR: {exc}", file=sys.stderr)
            return 2
        if hits:
            bad = True
            print(f"FOUND in {name}: {', '.join(hits)}", file=sys.stderr)
        else:
            print(f"clean: {name}")
    return 1 if bad else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
