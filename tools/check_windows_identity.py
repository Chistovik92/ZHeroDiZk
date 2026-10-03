#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Verify the Windows client identity from the built files, not from source text."""
import json
import sys
from pathlib import Path

EXPECTED = {
    "ProductName": "ZHeroDiZk",
    "InternalName": "zherodizk",
    "OriginalFilename": "zherodizk.exe",
    "CompanyName": "SecretHero",
}


def validate(version_info, filenames):
    names = {name.lower() for name in filenames}
    if "zherodizk.exe" not in names:
        raise ValueError("zherodizk.exe is missing from the output")
    if "rustdesk.exe" in names:
        raise ValueError("Output still contains the upstream executable name")
    for key, value in EXPECTED.items():
        if version_info.get(key) != value:
            raise ValueError(f"Executable metadata {key} is {version_info.get(key)!r}, expected {value!r}")
    if "ZHeroDiZk" not in (version_info.get("FileDescription") or ""):
        raise ValueError("Executable description does not carry the product name")
    return True


def main(argv):
    try:
        info = json.loads(Path(argv[1]).read_text(encoding="utf-8-sig"))
        validate(info, [p.name for p in Path(argv[2]).iterdir()])
        print("Windows identity OK")
        return 0
    except (OSError, ValueError, IndexError, TypeError, AttributeError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
