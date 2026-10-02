# SPDX-License-Identifier: AGPL-3.0-only
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "packaging"
UNITS = {"zhd-rendezvous": "ZHD_RENDEZVOUS_ARGS", "zhd-relay": "ZHD_RELAY_ARGS"}
HARDENING = ["NoNewPrivileges=true", "ProtectSystem=strict", "ProtectHome=true", "PrivateTmp=true",
             "User=zherodizk", "ReadWritePaths=/var/lib/zherodizk", "RestrictAddressFamilies="]


def text(*parts):
    return ROOT.joinpath(*parts).read_text(encoding="utf-8")


class PackagingTests(unittest.TestCase):
    def test_units_are_hardened_and_use_their_own_arguments(self):
        for name, variable in UNITS.items():
            unit = text("systemd", f"{name}.service")
            for line in HARDENING:
                self.assertIn(line, unit, f"{name}: {line}")
            self.assertIn(f"ExecStart=/usr/bin/{name} ${variable}", unit)
            self.assertIn("EnvironmentFile=-/etc/zherodizk/server.env", unit)

    def test_env_file_defines_both_variables(self):
        env = text("common", "server.env")
        for variable in UNITS.values():
            self.assertRegex(env, rf"(?m)^{variable}=")

    def test_packages_ship_the_same_files(self):
        spec = text("rpm", "zherodizk-server.spec")
        deb = text("build-deb.sh")
        for binary in ("zhd-rendezvous", "zhd-relay", "zhd-utils"):
            self.assertIn(binary, spec)
            self.assertIn(binary, deb)
        for unit in UNITS:
            self.assertIn(f"{unit}.service", spec)

    def test_no_third_party_names_in_packaging(self):
        for path in ROOT.rglob("*"):
            if path.is_file():
                self.assertNotIn("rustdesk", path.read_text(encoding="utf-8").lower(), str(path))

    def test_scripts_are_posix_shell_with_strict_mode(self):
        for name in ("build-deb.sh", "build-rpm.sh", "deb/postinst", "deb/prerm"):
            body = text(*name.split("/"))
            self.assertTrue(body.startswith("#!/bin/sh"), name)
            self.assertIn("set -e", body)

    def test_compose_drops_privileges(self):
        compose = text("docker", "docker-compose.yml")
        for needle in ("read_only: true", "cap_drop: [ALL]", "no-new-privileges"):
            self.assertIn(needle, compose)


if __name__ == "__main__":
    unittest.main()
