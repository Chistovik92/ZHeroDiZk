# SPDX-License-Identifier: AGPL-3.0-only
import hashlib
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
    def test_relative_bundle_uses_absolute_executable_path(self):
        calls = []
        with patch.object(Path, "is_file", return_value=True):
            self.assertEqual(launch([], "relative bundle", lambda _: 0,
                                   lambda *a: calls.append(a)), 0)
        binary = str((Path("relative bundle") / "zherodizk").resolve())
        self.assertEqual(calls, [(binary, [binary])])

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

    def test_preserves_crlf_line_endings(self):
        bootstrap.git(self.source, "config", "core.autocrlf", "false")
        cargo = self.source / "Cargo.toml"
        cargo.write_bytes(b'crate-type = ["cdylib", "staticlib", "rlib"]\r\n')
        self.commit(self.source)
        lock_path = self.root / "upstream.lock.json"
        lock = json.loads(lock_path.read_text())
        lock["client"]["commit"] = bootstrap.git(self.source, "rev-parse", "HEAD")
        lock_path.write_text(json.dumps(lock))
        prepare_client.prepare(self.source)
        self.assertEqual(cargo.read_bytes(), b'crate-type = ["cdylib"]\r\n')

    def test_write_failure_restores_changed_files(self):
        self.identity_fixture()
        real_write = prepare_client.write_source
        calls = []

        def flaky(path, text):
            calls.append(path)
            if len(calls) == 2:
                raise OSError("disk full")
            real_write(path, text)

        with patch.object(prepare_client, "write_source", flaky):
            with self.assertRaises(OSError):
                prepare_client.prepare(self.source, "zherodizk")
        self.assertEqual(bootstrap.git(self.source, "status", "--porcelain"), "")

    def test_rejects_wrong_commit(self):
        (self.source / "new-file").write_text("changed revision")
        self.commit(self.source)
        with self.assertRaisesRegex(ValueError, "match"):
            prepare_client.prepare(self.source)

    def test_rejects_uninitialized_submodule(self):
        bootstrap.git(self.source, "submodule", "deinit", "--all")
        with self.assertRaisesRegex(ValueError, "submodules"):
            prepare_client.prepare(self.source)

    def identity_fixture(self, platform="linux"):
        client_dir = Path(__file__).resolve().parents[1] / "client"
        (self.root / "client").mkdir(exist_ok=True)
        contents = {}
        for name in ("common", platform):
            data = json.loads((client_dir / f"{name}-identity.json").read_text(encoding="utf-8"))
            for item in data.get("files", []):
                # Stand-in for the upstream file, with its hash written into the manifest copy.
                target = self.source / item["path"]
                target.parent.mkdir(parents=True, exist_ok=True)
                stand_in = bytes([85, 13, 10, 86, 10]) if item.get("normalize_eol") else b"\x00upstream"
                target.write_bytes(stand_in)
                compared = stand_in.replace(bytes([13, 10]), bytes([10])) if item.get("normalize_eol") else stand_in
                item["sha256_before"] = hashlib.sha256(compared).hexdigest()
                replacement = self.root / item["source"]
                replacement.parent.mkdir(parents=True, exist_ok=True)
                replacement.write_bytes((client_dir.parent / item["source"]).read_bytes())
            (self.root / "client" / f"{name}-identity.json").write_text(json.dumps(data), encoding="utf-8")
            for change in data["changes"]:
                contents.setdefault(change["path"], []).append(change["before"])
            if name == platform:
                manifest = data
        for name, fragments in contents.items():
            target = self.source / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("\n".join(fragments) + "\n", encoding="utf-8")
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

    def test_common_manifest_removes_third_party_defaults(self):
        self.identity_fixture()
        prepare_client.prepare(self.source, "zherodizk")
        config = (self.source / "libs/hbb_common/src/config.rs").read_text(encoding="utf-8")
        self.assertIn('RS_PUB_KEY: &str = ""', config)
        self.assertIn("unconfigured.zherodizk.invalid", config)
        self.assertNotIn("rustdesk.com", config)
        common = (self.source / "src/common.rs").read_text(encoding="utf-8")
        self.assertIn("if true {", common)
        self.assertNotIn("admin.rustdesk.com", common)

    def test_common_manifest_shape(self):
        data = json.loads((Path(__file__).resolve().parents[1] / "client/common-identity.json").read_text(encoding="utf-8"))
        self.assertEqual(data["platform"], "common")
        for change in data["changes"]:
            self.assertNotIn(chr(10), change["before"], "anchors must be single-line (CRLF checkouts)")
            self.assertNotIn("rustdesk.com", change["after"])

    def test_branding_files_replace_upstream_icons(self):
        self.identity_fixture("windows")
        report = prepare_client.prepare(self.source, "zherodizk", "windows")
        icon = (self.source / "flutter/windows/runner/resources/app_icon.ico").read_bytes()
        self.assertEqual(icon, (Path(__file__).resolve().parents[1] / "client/branding/app_icon.ico").read_bytes())
        self.assertIn("flutter/assets/icon.svg", report["changed_files"])
        self.assertIn(b"<svg", (self.source / "flutter/assets/icon.svg").read_bytes())

    def test_changed_upstream_icon_is_refused_without_writing_anything(self):
        self.identity_fixture("windows")
        (self.source / "flutter/windows/runner/resources/app_icon.ico").write_bytes(b"someone else's icon")
        self.commit(self.source)
        lock_path = self.root / "upstream.lock.json"
        lock = json.loads(lock_path.read_text(encoding="utf-8"))
        lock["client"]["commit"] = bootstrap.git(self.source, "rev-parse", "HEAD")
        lock_path.write_text(json.dumps(lock), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Upstream file changed"):
            prepare_client.prepare(self.source, "zherodizk", "windows")
        self.assertEqual(bootstrap.git(self.source, "status", "--porcelain"), "")

    def test_failed_file_write_restores_text_and_binary_files(self):
        self.identity_fixture("windows")
        icon = self.source / "flutter/windows/runner/resources/app_icon.ico"
        before = icon.read_bytes()
        real_write_bytes = Path.write_bytes

        def failing(path, data):
            if path.name == "app_icon.ico" and data != before:
                raise OSError("disk full")
            return real_write_bytes(path, data)

        with patch.object(Path, "write_bytes", failing):
            with self.assertRaises(OSError):
                prepare_client.prepare(self.source, "zherodizk", "windows")
        self.assertEqual(bootstrap.git(self.source, "status", "--porcelain"), "")
        self.assertEqual(icon.read_bytes(), before)

    def test_android_identity_changes_application_id_and_labels(self):
        self.identity_fixture("android")
        report = prepare_client.prepare(self.source, "zherodizk", "android")
        self.assertEqual(report["platform"], "android")
        gradle = (self.source / "flutter/android/app/build.gradle").read_text(encoding="utf-8")
        self.assertIn("io.github.chistovik92.zherodizk", gradle)
        self.assertNotIn("com.carriez.flutter_hbb", gradle)
        manifest = (self.source / "flutter/android/app/src/main/AndroidManifest.xml").read_text(encoding="utf-8")
        self.assertIn('android:label="ZHeroDiZk"', manifest)
        self.assertIn('android:label="ZHeroDiZk Input"', manifest)
        self.assertEqual(report["adjustments"], [])
        self.assertNotIn("Cargo.toml", report["changed_files"])

    def test_windows_identity_leaves_cargo_untouched(self):
        self.identity_fixture("windows")
        report = prepare_client.prepare(self.source, "zherodizk", "windows")
        self.assertEqual(report["platform"], "windows")
        self.assertEqual(report["adjustments"], [])
        self.assertNotIn("Cargo.toml", report["changed_files"])
        cmake = (self.source / "flutter/windows/CMakeLists.txt").read_text(encoding="utf-8")
        self.assertIn('set(BINARY_NAME "zherodizk")', cmake)
        rc = (self.source / "flutter/windows/runner/Runner.rc").read_text(encoding="utf-8")
        self.assertIn('"ProductName", "ZHeroDiZk"', rc)
        self.assertNotIn("Purslane", rc)

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


class ManifestFileEntryTests(unittest.TestCase):
    def test_file_entries_are_well_formed(self):
        client = Path(__file__).resolve().parents[1] / "client"
        for name in ("common", "linux", "windows", "android"):
            manifest = json.loads((client / f"{name}-identity.json").read_text(encoding="utf-8"))
            for item in manifest.get("files", []):
                self.assertRegex(item["sha256_before"], "^[0-9a-f]{64}$", item["path"])
                self.assertTrue((client.parent / item["source"]).is_file(), item["source"])
                self.assertNotIn("..", item["path"])
