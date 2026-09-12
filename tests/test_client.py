# SPDX-License-Identifier: AGPL-3.0-only
import tempfile
import unittest
from pathlib import Path
from tools.launch_client import launch


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
