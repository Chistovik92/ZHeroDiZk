#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Read-only diagnostics for a Linux X11 remote host. Python >= 3.10."""
import argparse
import ctypes
import json
import os
import platform
import subprocess
import sys
from pathlib import Path

SYMBOLS = ("xdo_new", "xdo_free", "xdo_move_mouse", "xdo_mouse_down", "xdo_mouse_up")


def probe_xdo(loader=ctypes.CDLL):
    # Load the exact SONAME: libxdot.so.4 is NOT a valid replacement.
    try:
        library = loader("libxdo.so.3")
        for symbol in SYMBOLS:
            getattr(library, symbol)
        return {"ok": True, "code": "xdo_loaded"}
    except (OSError, AttributeError) as exc:
        return {"ok": False, "code": "xdo_unavailable", "detail": str(exc)}


def isolated_probe():
    try:
        result = subprocess.run(
            [sys.executable, str(Path(__file__).resolve()), "--probe-xdo"],
            capture_output=True, text=True, timeout=5, check=False,
        )
        if result.returncode != 0:
            return {"ok": False, "code": "xdo_probe_failed"}
        return json.loads(result.stdout)
    except (OSError, subprocess.TimeoutExpired, ValueError):
        return {"ok": False, "code": "xdo_probe_failed"}


def diagnose(system, session, display, uid, probe):
    checks = []
    def add(code, level, message):
        checks.append({"code": code, "level": level, "message": message})
    if system != "Linux":
        add("unsupported_os", "error", "Эта диагностика предназначена для Linux.")
        return checks
    if uid == 0:
        add("root_session", "warning", "Запустите диагностику из рабочего стола обычного пользователя.")
    if session == "x11":
        add("session_x11", "ok", "Графический сеанс X11.")
        if not display:
            add("display_missing", "error", "Нет DISPLAY: откройте терминал в графическом сеансе.")
        else:
            add("display_set", "ok", "Переменная DISPLAY задана; доступ к X-серверу ещё не проверен.")
        if probe.get("ok"):
            add("xdo_loaded", "ok", "libxdo.so.3 загружается, функции мыши найдены.")
        else:
            add("xdo_unavailable", "error", "libxdo.so.3 не загружается или не содержит нужных функций. В Simply/ALT установите xdotool и перезапустите приложение.")
    elif session == "wayland":
        add("wayland_unverified", "warning", "Wayland: требуется отдельная проверка разрешений захвата и ввода. Отсутствие libxdo здесь не считается ошибкой.")
    else:
        add("session_unknown", "error", "Не определён графический сеанс. Запустите из терминала рабочего стола.")
    return checks


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="машиночитаемый отчёт")
    parser.add_argument("--probe-xdo", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.probe_xdo:
        print(json.dumps(probe_xdo()))
        return 0
    system = platform.system()
    session = os.environ.get("XDG_SESSION_TYPE", "").lower()
    probe = isolated_probe() if system == "Linux" and session == "x11" else {}
    checks = diagnose(system, session, os.environ.get("DISPLAY", ""),
                      os.geteuid() if hasattr(os, "geteuid") else -1, probe)
    exit_code = 2 if any(c["level"] == "error" for c in checks) else (1 if any(c["level"] == "warning" for c in checks) else 0)
    report = {"schema_version": 1, "checks": checks, "exit_code": exit_code,
              "scope": "environment-only; does not prove input or capture works"}
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        for check in checks:
            print(f'[{check["level"].upper()}] {check["message"]}')
        print("Проверка библиотек не заменяет испытание реального подключения.")
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
