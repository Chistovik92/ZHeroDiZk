# SPDX-License-Identifier: AGPL-3.0-only
import json
import tempfile
import unittest
from pathlib import Path

from tools import check_windows_identity as check
from tools import prepare_client

INFO = {"ProductName": "ZHeroDiZk", "InternalName": "zherodizk",
        "OriginalFilename": "zherodizk.exe", "CompanyName": "SecretHero",
        "FileDescription": "ZHeroDiZk Remote Desktop"}
FILES = ["zherodizk.exe", "librustdesk.dll", "data"]


class WindowsIdentityTests(unittest.TestCase):
    def test_accepts_expected_output(self):
        self.assertTrue(check.validate(INFO, FILES))

    def test_rejects_upstream_executable(self):
        with self.assertRaisesRegex(ValueError, "upstream"):
            check.validate(INFO, FILES + ["RustDesk.exe"])

    def test_rejects_missing_executable(self):
        with self.assertRaisesRegex(ValueError, "missing"):
            check.validate(INFO, ["librustdesk.dll"])

    def test_rejects_wrong_metadata(self):
        for key in ("ProductName", "OriginalFilename", "CompanyName"):
            bad = dict(INFO, **{key: "Other"})
            with self.assertRaisesRegex(ValueError, key):
                check.validate(bad, FILES)

    def test_cli_exit_codes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "out").mkdir()
            (root / "out/zherodizk.exe").touch()
            (root / "info.json").write_text(json.dumps(INFO), encoding="utf-8")
            self.assertEqual(check.main(["x", str(root / "info.json"), str(root / "out")]), 0)
            self.assertEqual(check.main(["x", str(root / "missing.json"), str(root / "out")]), 2)

    def test_windows_manifest_is_valid_and_unique(self):
        manifest = json.loads((Path(__file__).resolve().parents[1] / "client/windows-identity.json").read_text(encoding="utf-8"))
        self.assertEqual((manifest["schema_version"], manifest["profile"], manifest["platform"]), (1, "zherodizk", "windows"))
        seen = set()
        for change in manifest["changes"]:
            key = (change["path"], change["before"])
            self.assertNotIn(key, seen)
            seen.add(key)
            self.assertNotEqual(change["before"], change["after"])
            self.assertNotIn("rustdesk", change["after"].lower())

    def test_unknown_platform_rejected(self):
        with self.assertRaisesRegex(ValueError, "platform"):
            prepare_client.prepare(Path("."), "zherodizk", "plan9")


if __name__ == "__main__":
    unittest.main()
