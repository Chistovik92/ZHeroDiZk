#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Verify the Linux identity emitted by the compiled client, not source text."""
import json
import sys
from pathlib import Path, PurePosixPath


def validate(report):
    if report.get("app_name") != "ZHeroDiZk":
        raise ValueError("Client reports an unexpected application name")
    config = PurePosixPath(report["config_file"])
    if not config.is_absolute() or config.name != "ZHeroDiZk.toml" or config.parent.name != "zherodizk":
        raise ValueError("Client config is not in the ZHeroDiZk namespace")
    log = PurePosixPath(report["log_dir"])
    if not log.is_absolute() or log.parts[-4:] != (".local", "share", "logs", "ZHeroDiZk"):
        raise ValueError("Client log directory is not isolated")
    ipc = PurePosixPath(report["ipc_path"])
    if not ipc.is_absolute() or len(ipc.parts) < 4 or ipc.parts[1] != "tmp" or not ipc.parts[2].startswith("ZHeroDiZk-"):
        raise ValueError("Client IPC is not in the ZHeroDiZk namespace")
    return report


if __name__ == "__main__":
    try:
        report = validate(json.loads(Path(sys.argv[1]).read_text()))
        print(json.dumps(report, ensure_ascii=False, indent=2))
    except (OSError, ValueError, KeyError, TypeError, IndexError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        raise SystemExit(2)
