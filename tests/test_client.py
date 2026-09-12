# SPDX-License-Identifier: AGPL-3.0-only
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from tools.launch_client import launch
from tools import bootstrap, prepare_client
from tools.apply_patch_once import apply_once
from tools.check_client_identity import validate


class LauncherTests(unittest.TestCase):
    def test_diagnostic_error_prevents_execution(self):
        calls = []
        self.assertEqual(launch([], check=lambda _: 2, execute=lambda *a: calls.append(a)), 2)
        self.assertEqual(calls, [])

    def test_arguments_and_paths_with_spaces_are_preserved(self):
        with tempfile.TemporaryDirectory(prefix="client bundle ") as temp:
            binary = Path(temp) / "zherodizk"
            binary.touch()
            calls = []
            self.assertEqual(launch(["--connect", "123 456"], temp,
                                   lambda _: 0, lambda *a: calls.append(a)), 0)
            self.assertEqual(calls, [(str(binary), [str(binary), "--connect", "123 456"])])

    def test_warning_does_not_block_viewer(self):
        with tempfile.TemporaryDirectory() as temp:
            (Path(temp) / "zherodizk").touch()
            calls = []
            self.assertEqual(launch([], temp, lambda _: 1, lambda *a: calls.append(a)), 0)
            self.assertEqual(len(calls), 1)

    def test_doctor_does_not_start_client(self):
        calls = []
        self.assertEqual(launch(["--doctor"], check=lambda _: 0,
                               execute=lambda *a: calls.append(a)), 0)
        self.assertEqual(calls, [])

    def test_missing_binary_is_error(self):
        with tempfile.TemporaryDirectory() as temp:
            self.assertEqual(launch([], temp, lambda _: 0), 2)

    def test_does_not_fall_back_to_rustdesk(self):
        with tempfile.TemporaryDirectory() as temp:
            (Path(temp) / "rustdesk").touch()
            calls = []
            self.assertEqual(launch([], temp, lambda _: 0, lambda *a: calls.append(a)), 2)
            self.assertEqual(calls, [])


class IdentityReportTests(unittest.TestCase):
    def report(self):
        return {"app_name": "ZHeroDiZk", "config_file": "/home/user/.config/zherodizk/ZHeroDiZk.toml",
                "log_dir": "/home/user/.local/share/logs/ZHeroDiZk", "ipc_path": "/tmp/ZHeroDiZk-1000/ipc"}

    def test_valid_runtime_report(self):
        self.assertEqual(validate(self.report()), self.report())

    def test_mixed_rustdesk_namespace_rejected(self):
        for key, value in self.report().items():
            report = self.report()
            report[key] = value.replace("ZHeroDiZk", "RustDesk").replace("zherodizk", "rustdesk")
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate(report)

    def test_incomplete_report_rejected(self):
        with self.assertRaises(KeyError):
            validate({"app_name": "ZHeroDiZk"})


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "source"
        self.source.mkdir()
        bootstrap.git(self.source, "init")
        (self.source / "Cargo.toml").write_text('crate-type = ["cdylib", "staticlib", "rlib"]\n')
        (self.source / "LICENCE").write_text("test fixture")
        self.commit(self.source)
        sub = self.root / "sub"
        sub.mkdir()
        bootstrap.git(sub, "init")
        (sub / "LICENSE").write_text("test fixture")
        self.commit(sub)
        bootstrap.git(self.source, "-c", "protocol.file.allow=always", "submodule", "add", str(sub), "libs/common")
        self.commit(self.source)
        self.sha = bootstrap.git(self.source, "rev-parse", "HEAD")
        (self.root / "upstream.lock.json").write_text(json.dumps({
            "schema_version": 1, "client": {
                "url": "https://github.com/rustdesk/rustdesk.git", "commit": self.sha}}))
        self.root_patch = patch.object(prepare_client, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)

    def commit(self, path):
        bootstrap.git(path, "add", ".")
        bootstrap.git(path, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "fixture")

    def test_prepares_pinned_checkout_and_refuses_second_pass(self):
        report = prepare_client.prepare(self.source)
        self.assertEqual(report["upstream"]["commit"], self.sha)
        self.assertEqual((self.source / "Cargo.toml").read_text(), 'crate-type = ["cdylib"]\n')
        with self.assertRaisesRegex(ValueError, "clean"):
            prepare_client.prepare(self.source)

    def test_preserves_existing_changes(self):
        cargo = self.source / "Cargo.toml"
        cargo.write_text("user changes")
        with self.assertRaisesRegex(ValueError, "clean"):
            prepare_client.prepare(self.source)
        self.assertEqual(cargo.read_text(), "user changes")

    def test_rejects_wrong_commit(self):
        (self.source / "new-file").write_text("changed revision")
        self.commit(self.source)
        with self.assertRaisesRegex(ValueError, "match"):
            prepare_client.prepare(self.source)

    def test_rejects_uninitialized_submodule(self):
        bootstrap.git(self.source, "submodule", "deinit", "--all")
        with self.assertRaisesRegex(ValueError, "submodules"):
            prepare_client.prepare(self.source)

    def identity_fixture(self):
        manifest_path = Path(__file__).resolve().parents[1] / "client/linux-identity.json"
        manifest = json.loads(manifest_path.read_text())
        (self.root / "client").mkdir()
        (self.root / "client/linux-identity.json").write_text(json.dumps(manifest))
        contents = {}
        for change in manifest["changes"]:
            contents.setdefault(change["path"], []).append(change["before"])
        for name, fragments in contents.items():
            target = self.source / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("\n".join(fragments) + "\n")
        self.commit(self.source)
        lock_path = self.root / "upstream.lock.json"
        lock = json.loads(lock_path.read_text())
        lock["client"]["commit"] = bootstrap.git(self.source, "rev-parse", "HEAD")
        lock_path.write_text(json.dumps(lock))
        return manifest

    def test_separate_linux_identity(self):
        self.identity_fixture()
        report = prepare_client.prepare(self.source, "zherodizk")
        self.assertEqual(report["identity"]["binary_name"], "zherodizk")
        config = (self.source / "libs/hbb_common/src/config.rs").read_text()
        self.assertIn('RwLock::new("ZHeroDiZk".to_owned())', config)
        self.assertNotIn('"RustDesk"', config)
        cmake = (self.source / "flutter/linux/CMakeLists.txt").read_text()
        self.assertIn('set(BINARY_NAME "zherodizk")', cmake)
        self.assertIn('"io.github.chistovik92.zherodizk"', cmake)
        self.assertEqual((self.source / "LICENCE").read_text(), "test fixture")

    def test_identity_mismatch_does_not_partially_patch_cargo(self):
        self.identity_fixture()
        cmake = self.source / "flutter/linux/CMakeLists.txt"
        cmake.write_text("changed upstream layout\n")
        self.commit(self.source)
        lock_path = self.root / "upstream.lock.json"
        lock = json.loads(lock_path.read_text())
        lock["client"]["commit"] = bootstrap.git(self.source, "rev-parse", "HEAD")
        lock_path.write_text(json.dumps(lock))
        before = bootstrap.git(self.source, "status", "--porcelain")
        with self.assertRaisesRegex(ValueError, "layout"):
            prepare_client.prepare(self.source, "zherodizk")
        self.assertEqual(bootstrap.git(self.source, "status", "--porcelain"), before)

    def test_identity_cannot_write_outside_checkout(self):
        manifest = self.identity_fixture()
        manifest["changes"][0]["path"] = "../outside.txt"
        (self.root / "client/linux-identity.json").write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "escapes"):
            prepare_client.prepare(self.source, "zherodizk")
        self.assertFalse((self.root / "outside.txt").exists())

    def test_unknown_profile_rejected(self):
        with self.assertRaisesRegex(ValueError, "Unknown"):
            prepare_client.prepare(self.source, "other")


class CachedSdkPatchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.sdk = self.root / "sdk"
        self.sdk.mkdir()
        bootstrap.git(self.sdk, "init")
        self.source = self.sdk / "source.txt"
        self.source.write_text("before\n")
        self.patch = self.root / "fix.diff"
        self.patch.write_text("--- a/source.txt\n+++ b/source.txt\n@@ -1 +1 @@\n-before\n+after\n")

    def test_fresh_and_cached_sdk(self):
        self.assertTrue(apply_once(self.sdk, self.patch))
        self.assertEqual(self.source.read_text(), "after\n")
        self.assertFalse(apply_once(self.sdk, self.patch))
        self.assertEqual(self.source.read_text(), "after\n")

    def test_conflict_fails_without_overwriting(self):
        self.source.write_text("unrelated edit\n")
        with self.assertRaises(subprocess.CalledProcessError):
            apply_once(self.sdk, self.patch)
        self.assertEqual(self.source.read_text(), "unrelated edit\n")
