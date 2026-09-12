# SPDX-License-Identifier: AGPL-3.0-only
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from tools.launch_client import launch
from tools import bootstrap, prepare_client


class LauncherTests(unittest.TestCase):
    def test_diagnostic_error_prevents_execution(self):
        calls = []
        self.assertEqual(launch([], check=lambda _: 2, execute=lambda *a: calls.append(a)), 2)
        self.assertEqual(calls, [])

    def test_arguments_and_paths_with_spaces_are_preserved(self):
        with tempfile.TemporaryDirectory(prefix="client bundle ") as temp:
            binary = Path(temp) / "rustdesk"
            binary.touch()
            calls = []
            self.assertEqual(launch(["--connect", "123 456"], temp,
                                   lambda _: 0, lambda *a: calls.append(a)), 0)
            self.assertEqual(calls, [(str(binary), [str(binary), "--connect", "123 456"])])

    def test_warning_does_not_block_viewer(self):
        with tempfile.TemporaryDirectory() as temp:
            (Path(temp) / "rustdesk").touch()
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
