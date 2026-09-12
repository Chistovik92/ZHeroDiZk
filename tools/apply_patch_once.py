#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Apply a build patch, accepting an already patched SDK restored from cache."""
import argparse
import subprocess
from pathlib import Path

if __package__:
    from .bootstrap import git
else:
    from bootstrap import git


def apply_once(checkout, patch_file):
    patch_file = str(Path(patch_file).resolve())
    try:
        git(checkout, "apply", "--check", patch_file)
    except subprocess.CalledProcessError:
        # Accept only a fully applied patch, not arbitrary SDK conflicts.
        git(checkout, "apply", "--reverse", "--check", patch_file)
        return False
    git(checkout, "apply", patch_file)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkout", type=Path)
    parser.add_argument("patch", type=Path)
    args = parser.parse_args()
    try:
        print("Patch applied" if apply_once(args.checkout, args.patch) else "Patch already applied")
        return 0
    except (OSError, subprocess.SubprocessError) as exc:
        print(f"ERROR: {exc}")
        if isinstance(exc, subprocess.CalledProcessError):
            print(exc.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
