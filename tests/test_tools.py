# SPDX-License-Identifier: AGPL-3.0-only
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from tools import doctor, bootstrap


class DoctorTests(unittest.TestCase):
    def test_exact_library_name_not_libxdot(self):
        names = []
        def load(name):
            names.append(name)
            raise OSError("missing")
        self.assertFalse(doctor.probe_xdo(load)["ok"])
        self.assertEqual(names, ["libxdo.so.3"])

    def test_library_must_export_mouse_functions(self):
        self.assertFalse(doctor.probe_xdo(lambda _: SimpleNamespace(xdo_new=1))["ok"])

    def test_valid_library(self):
        library = SimpleNamespace(**{name: object() for name in doctor.SYMBOLS})
        self.assertTrue(doctor.probe_xdo(lambda _: library)["ok"])

    def test_missing_xdo_blocks_x11_readiness(self):
        checks = doctor.diagnose("Linux", "x11", ":0", 1000, {"ok": False})
        self.assertIn("xdo_unavailable", [c["code"] for c in checks if c["level"] == "error"])

    def test_wayland_does_not_require_xdo(self):
        checks = doctor.diagnose("Linux", "wayland", "", 1000, {"ok": False})
        self.assertEqual([c["code"] for c in checks], ["wayland_unverified"])

    def test_root_is_not_silently_approved(self):
        checks = doctor.diagnose("Linux", "x11", ":0", 0, {"ok": True})
        self.assertIn("root_session", [c["code"] for c in checks])

    def test_missing_display_is_error(self):
        checks = doctor.diagnose("Linux", "x11", "", 1000, {"ok": True})
        self.assertIn("display_missing", [c["code"] for c in checks if c["level"] == "error"])

    def test_unsupported_platform_not_ready(self):
        checks = doctor.diagnose("Windows", "", "", -1, {})
        self.assertEqual(checks[0]["level"], "error")

    def test_unknown_session_not_ready(self):
        checks = doctor.diagnose("Linux", "", ":0", 1000, {"ok": True})
        self.assertIn("session_unknown", [c["code"] for c in checks])

    def test_cli_json_and_exit_code(self):
        env = dict(os.environ, XDG_SESSION_TYPE="", DISPLAY="")
        result = subprocess.run([sys.executable, "tools/doctor.py", "--json"],
                                capture_output=True, text=True, env=env)
        report = json.loads(result.stdout)
        self.assertEqual(report["exit_code"], result.returncode)
        self.assertEqual(result.returncode, 2)

    def test_native_probe_is_isolated(self):
        result = doctor.isolated_probe()
        self.assertIsInstance(result["ok"], bool)


class BootstrapTests(unittest.TestCase):
    def lock(self):
        return {"schema_version": 1, "client": {
            "url": "https://github.com/rustdesk/rustdesk.git", "commit": "a" * 40}}

    def test_rejects_mutable_revision(self):
        lock = self.lock(); lock["client"]["commit"] = "master"
        with self.assertRaises(ValueError): bootstrap.validate_lock(lock)

    def test_rejects_unexpected_remote(self):
        lock = self.lock(); lock["client"]["url"] = "https://example.com/code.git"
        with self.assertRaises(ValueError): bootstrap.validate_lock(lock)

    def test_rejects_unknown_schema(self):
        lock = self.lock(); lock["schema_version"] = 2
        with self.assertRaises(ValueError): bootstrap.validate_lock(lock)

    def test_real_checkout_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as temp:
            origin = Path(temp) / "origin"; origin.mkdir()
            bootstrap.git(origin, "init")
            (origin / "LICENCE").write_text("fixture only")
            bootstrap.git(origin, "add", "LICENCE")
            bootstrap.git(origin, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "fixture")
            sha = bootstrap.git(origin, "rev-parse", "HEAD")
            client = {"url": str(origin), "commit": sha}
            dest = Path(temp) / "checkout with spaces"
            self.assertEqual(bootstrap.checkout(client, dest), sha)
            (dest / "user-work").write_text("preserve")
            with self.assertRaises(FileExistsError): bootstrap.checkout(client, dest)
            self.assertEqual((dest / "user-work").read_text(), "preserve")

    def test_existing_empty_directory_is_not_reused(self):
        with tempfile.TemporaryDirectory() as temp:
            with self.assertRaises(FileExistsError):
                bootstrap.checkout(self.lock()["client"], Path(temp))


if __name__ == "__main__":
    unittest.main()
