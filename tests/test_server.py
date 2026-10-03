# SPDX-License-Identifier: AGPL-3.0-only
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import bootstrap, prepare_server

REAL_MANIFEST = Path(__file__).resolve().parents[1] / "server" / "identity.json"


class ServerIdentityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "source"
        self.source.mkdir()
        bootstrap.git(self.source, "init")
        bootstrap.git(self.source, "config", "core.autocrlf", "false")
        self.manifest = json.loads(REAL_MANIFEST.read_text(encoding="utf-8"))
        files = {}
        for change in self.manifest["changes"]:
            files.setdefault(change["path"], []).append(change["before"])
        for name, fragments in files.items():
            target = self.source / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(("\n".join(fragments) + "\n").encode("utf-8"))
        sub = self.root / "sub"
        sub.mkdir()
        bootstrap.git(sub, "init")
        (sub / "f").write_text("x")
        self.commit(sub)
        self.commit(self.source)
        bootstrap.git(self.source, "-c", "protocol.file.allow=always", "submodule", "add", str(sub), "libs/common")
        self.commit(self.source)
        sha = bootstrap.git(self.source, "rev-parse", "HEAD")
        (self.root / "server").mkdir()
        (self.root / "server/identity.json").write_text(json.dumps(self.manifest), encoding="utf-8")
        (self.root / "upstream.lock.json").write_text(json.dumps({
            "schema_version": 1,
            "client": {"url": "https://github.com/rustdesk/rustdesk.git", "commit": "a" * 40},
            "server": {"url": "https://github.com/rustdesk/rustdesk-server.git", "commit": sha}}), encoding="utf-8")
        patcher = patch.object(prepare_server, "ROOT", self.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def commit(self, path):
        bootstrap.git(path, "add", ".")
        bootstrap.git(path, "-c", "user.name=T", "-c", "user.email=t@example.invalid", "commit", "-m", "f")

    def test_real_manifest_removes_third_party_names_and_phone_home(self):
        for change in self.manifest["changes"]:
            self.assertNotIn("rustdesk", change["after"].lower())
        calls = [c for c in self.manifest["changes"] if "check_software_update" in c["before"]]
        self.assertEqual(len(calls), 1)
        self.assertNotIn("check_software_update", calls[0]["after"])

    def test_applies_identity_and_reports_binaries(self):
        report = prepare_server.prepare(self.source)
        self.assertEqual(report["binaries"]["relay"], "zhd-relay")
        cargo = (self.source / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('name = "zhd-rendezvous"', cargo)
        self.assertIn("autobins = false", cargo)
        self.assertNotIn("rustdesk", cargo.lower())

    def test_refuses_dirty_tree_and_wrong_commit(self):
        (self.source / "Cargo.toml").write_text("changed", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "clean"):
            prepare_server.prepare(self.source)
        self.commit(self.source)
        with self.assertRaisesRegex(ValueError, "match"):
            prepare_server.prepare(self.source)

    def test_layout_change_is_rejected_without_partial_write(self):
        (self.source / "src/utils.rs").write_bytes(b"different layout\n")
        self.commit(self.source)
        lock_path = self.root / "upstream.lock.json"
        lock = json.loads(lock_path.read_text(encoding="utf-8"))
        lock["server"]["commit"] = bootstrap.git(self.source, "rev-parse", "HEAD")
        lock_path.write_text(json.dumps(lock), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "layout"):
            prepare_server.prepare(self.source)
        self.assertEqual(bootstrap.git(self.source, "status", "--porcelain"), "")


if __name__ == "__main__":
    unittest.main()
