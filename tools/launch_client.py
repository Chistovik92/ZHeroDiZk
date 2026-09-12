#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Interactive development launcher; does not install a system service."""
import os
import sys
from pathlib import Path

if __package__:
    from . import doctor
else:
    import doctor


def launch(argv=None, bundle=None, check=None, execute=None):
    argv = sys.argv[1:] if argv is None else argv
    bundle = Path(__file__).resolve().parent if bundle is None else Path(bundle)
    check = doctor.main if check is None else check
    execute = os.execv if execute is None else execute
    if argv == ["--doctor"]:
        return check([])
    status = check([])
    if status == 2:
        print("ZHeroDiZk: запуск остановлен. Исправьте ошибки диагностики выше.", file=sys.stderr)
        return status
    binary = bundle / "zherodizk"
    if not binary.is_file():
        print(f"Не найден клиент: {binary}", file=sys.stderr)
        return 2
    print("ZHeroDiZk: экспериментальная сборка на базе RustDesk; собственные правила доступа ещё не подключены.", flush=True)
    sys.stdout.flush()
    sys.stderr.flush()
    try:
        execute(str(binary), [str(binary), *argv])
    except OSError as exc:
        print(f"Не удалось запустить клиент: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(launch())
