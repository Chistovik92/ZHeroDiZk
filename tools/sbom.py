#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Build a CycloneDX 1.5 component list and a licence summary.

Rust components come from `cargo metadata --format-version 1 --locked` output;
Dart components come from pubspec.lock; optional package_config.json and vcpkg
installed trees supply the actual licence texts without guessing SPDX identifiers.
"""
import argparse
import base64
import json
import re
import sys
from collections import Counter
from pathlib import Path
from urllib.parse import urljoin, urlparse
from urllib.request import url2pathname

UNKNOWN = "UNKNOWN"


def license_files(root):
    result = []
    for path in sorted(root.iterdir()):
        if path.is_file() and path.name.lower().split('.')[0] in ("license", "licence", "copying", "copyright"):
            result.append({"license": {"name": path.name,
                           "text": {"contentType": "text/plain", "encoding": "base64",
                                    "content": base64.b64encode(path.read_bytes()).decode("ascii")}}})
    return result


def collect_dart_licenses(components, config_path):
    config_path = config_path.resolve()
    config = json.loads(config_path.read_text(encoding="utf-8"))
    if config.get("configVersion") != 2:
        raise ValueError("Unsupported Dart package config")
    packages = {p["name"]: p for p in config["packages"]}
    for component in components:
        package = packages.get(component["name"])
        if package is None:
            raise ValueError(f"Dart package missing from config: {component['name']}")
        uri = urlparse(urljoin(config_path.as_uri(), package["rootUri"]))
        if uri.scheme != "file" or uri.netloc not in ("", "localhost"):
            raise ValueError("Dart package root must be a local file URI")
        root = Path(url2pathname(uri.path))
        licenses = license_files(root)
        if licenses:
            component["license"] = "SEE-LICENSE-FILE"
            component["licenses"] = licenses
    return components


def vcpkg_components(installed):
    components = []
    status = (installed / "vcpkg/status").read_text(encoding="utf-8")
    for paragraph in re.split(r"\n\s*\n", status):
        fields = dict(line.split(": ", 1) for line in paragraph.splitlines()
                      if ": " in line and not line.startswith(" "))
        if fields.get("Status") != "install ok installed" or "Feature" in fields:
            continue
        name, version, triplet = fields["Package"], fields["Version"], fields["Architecture"]
        if not all(re.fullmatch(r"[A-Za-z0-9_-]+", value) for value in (name, triplet)):
            raise ValueError("vcpkg package path escapes installed tree")
        root = (installed / triplet / "share" / name).resolve()
        if not root.is_relative_to(installed.resolve()):
            raise ValueError("vcpkg package path escapes installed tree")
        licenses = license_files(root) if root.is_dir() else []
        components.append({"type": "library", "name": name, "version": version,
                           "purl": f"pkg:generic/{name}@{version}?arch={triplet}",
                           "ecosystem": "vcpkg", "source": "installed",
                           "license": "SEE-LICENSE-FILE" if licenses else UNKNOWN,
                           "licenses": licenses})
    return components


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
                        "licenses": c.get("licenses", [{"license": {"name": "SEE-LICENSE-FILE"}}]
                                           if c["license"] == "SEE-LICENSE-FILE" else
                                           [{"expression": c["license"]}] if c["license"] != UNKNOWN else [])}
                       for c in ordered],
        "zherodizk": {
            "component_count": len(ordered),
            "included_ecosystems": sorted({c["ecosystem"] for c in ordered}),
            "license_summary": dict(sorted(summary.items())),
            "unknown_license_count": summary.get(UNKNOWN, 0),
            "notes": ["Licence texts are included when supplied package roots contain them; missing licences stay UNKNOWN.",
                      "Collected licence texts require review; no SPDX identifier is inferred from text."],
        },
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-metadata", type=Path, help="JSON from cargo metadata")
    parser.add_argument("--pubspec-lock", type=Path)
    parser.add_argument("--dart-package-config", type=Path)
    parser.add_argument("--vcpkg-installed", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    if not args.cargo_metadata and not args.pubspec_lock and not args.vcpkg_installed:
        parser.error("give --cargo-metadata, --pubspec-lock and/or --vcpkg-installed")
    if args.dart_package_config and not args.pubspec_lock:
        parser.error("--dart-package-config requires --pubspec-lock")
    try:
        components = []
        if args.cargo_metadata:
            components += rust_components(json.loads(args.cargo_metadata.read_text(encoding="utf-8")))
        if args.pubspec_lock:
            dart = dart_components(args.pubspec_lock.read_text(encoding="utf-8"))
            if args.dart_package_config:
                collect_dart_licenses(dart, args.dart_package_config)
            components += dart
        if args.vcpkg_installed:
            components += vcpkg_components(args.vcpkg_installed)
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
