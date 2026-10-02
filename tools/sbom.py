#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Build a CycloneDX 1.5 component list and a licence summary.

Rust components come from `cargo metadata --format-version 1 --locked` output;
Dart components come from pubspec.lock. Dart licences are NOT collected, and the
report says so instead of guessing.
"""
import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

UNKNOWN = "UNKNOWN"


def rust_components(metadata):
    components = []
    for pkg in metadata.get("packages", []):
        license_id = (pkg.get("license") or "").strip()
        if not license_id and pkg.get("license_file"):
            license_id = "SEE-LICENSE-FILE"
        components.append({
            "type": "library", "name": pkg["name"], "version": pkg["version"],
            "purl": f"pkg:cargo/{pkg['name']}@{pkg['version']}",
            "ecosystem": "cargo", "license": license_id or UNKNOWN,
            "source": pkg.get("source") or "local",
        })
    return components


def dart_components(lock_text):
    """Parse the fixed layout of pubspec.lock without a YAML dependency."""
    components = []
    in_packages = False
    current = None
    for raw in lock_text.splitlines():
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        if not raw.startswith(" "):
            in_packages = raw.strip() == "packages:"
            current = None
            continue
        if not in_packages:
            continue
        indent = len(raw) - len(raw.lstrip())
        text = raw.strip()
        if indent == 2 and text.endswith(":"):
            current = {"name": text[:-1], "version": None, "source": None}
            components.append(current)
        elif indent == 4 and current is not None:
            match = re.fullmatch(r'(version|source):\s*"?([^"]*)"?', text)
            if match:
                current[match.group(1)] = match.group(2)
    result = []
    for item in components:
        if not item["version"]:
            raise ValueError(f"pubspec.lock entry without version: {item['name']}")
        result.append({
            "type": "library", "name": item["name"], "version": item["version"],
            "purl": f"pkg:pub/{item['name']}@{item['version']}",
            "ecosystem": "pub", "license": UNKNOWN, "source": item["source"] or "unknown",
        })
    return result


def build_report(components):
    ordered = sorted(components, key=lambda c: (c["ecosystem"], c["name"], c["version"]))
    summary = Counter(c["license"] for c in ordered)
    return {
        "bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
        "components": [{"type": c["type"], "name": c["name"], "version": c["version"],
                        "purl": c["purl"],
                        "licenses": [{"expression": c["license"]}] if c["license"] != UNKNOWN else []}
                       for c in ordered],
        "zherodizk": {
            "component_count": len(ordered),
            "license_summary": dict(sorted(summary.items())),
            "unknown_license_count": summary.get(UNKNOWN, 0),
            "notes": ["Dart licences are not collected (pubspec.lock has no licence data).",
                      "Native libraries built via vcpkg are not included."],
        },
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-metadata", type=Path, help="JSON from cargo metadata")
    parser.add_argument("--pubspec-lock", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    if not args.cargo_metadata and not args.pubspec_lock:
        parser.error("give --cargo-metadata and/or --pubspec-lock")
    try:
        components = []
        if args.cargo_metadata:
            components += rust_components(json.loads(args.cargo_metadata.read_text(encoding="utf-8")))
        if args.pubspec_lock:
            components += dart_components(args.pubspec_lock.read_text(encoding="utf-8"))
        report = build_report(components)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        info = report["zherodizk"]
        print(f"{info['component_count']} components, {info['unknown_license_count']} with unknown licence")
        return 0
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
