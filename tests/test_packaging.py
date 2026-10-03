# SPDX-License-Identifier: AGPL-3.0-only
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "packaging"
UNITS = {"zhd-rendezvous": "ZHD_RENDEZVOUS_ARGS", "zhd-relay": "ZHD_RELAY_ARGS"}
HARDENING = ["UMask=0077", "NoNewPrivileges=true", "ProtectSystem=strict", "ProtectHome=true", "PrivateTmp=true",
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


CONTROL = ROOT / "control"


class ControlPackagingTests(unittest.TestCase):
    def test_unit_is_strongly_sandboxed(self):
        unit = (CONTROL / "zherodizk-control.service").read_text(encoding="utf-8")
        for line in ("User=zherodizk-control", "UMask=0077", "NoNewPrivileges=true", "ProtectSystem=strict",
                     "CapabilityBoundingSet=", "MemoryDenyWriteExecute=true", "SystemCallFilter=@system-service",
                     "EnvironmentFile=/etc/zherodizk/control.env", "ExecStart=/usr/bin/zherodizk-control"):
            self.assertIn(line, unit)
        self.assertNotIn("ReadWritePaths", unit, "the control server keeps no local state")

    def test_env_template_lists_every_setting_the_server_reads(self):
        env = (CONTROL / "control.env").read_text(encoding="utf-8")
        config = (ROOT.parent / "crates/control/src/config.rs").read_text(encoding="utf-8")
        for variable in ("ZHD_DATABASE_URL", "ZHD_LISTEN", "ZHD_ALLOW_REGISTRATION", "ZHD_AUTH_RATE_LIMIT",
                         "ZHD_TRUST_FORWARDED_FOR", "ZHD_MFA_KEY", "ZHD_GRANT_KEY"):
            self.assertRegex(env, rf"(?m)^{variable}=", variable)
            self.assertIn(f'"{variable}"', config, f"{variable} is not read by the server")

    def test_defaults_are_the_safe_ones(self):
        env = (CONTROL / "control.env").read_text(encoding="utf-8")
        self.assertIn("ZHD_LISTEN=127.0.0.1:", env)
        self.assertIn("ZHD_ALLOW_REGISTRATION=false", env)
        self.assertIn("ZHD_TRUST_FORWARDED_FOR=false", env)
        self.assertIn("CHANGE_ME", env, "the template must not ship a usable password")

    def test_packages_install_the_same_files_with_restricted_secrets(self):
        spec = (CONTROL / "zherodizk-control.spec").read_text(encoding="utf-8")
        deb = (ROOT / "build-control-deb.sh").read_text(encoding="utf-8")
        postinst = (CONTROL / "postinst").read_text(encoding="utf-8")
        self.assertIn("%attr(0640,root,zherodizk-control) /etc/zherodizk/control.env", spec)
        self.assertIn("install -m 0640", deb)
        self.assertIn("chmod 0640 /etc/zherodizk/control.env", postinst)
        for text in (spec, deb):
            self.assertIn("zherodizk-control.service", text)
            self.assertIn("Caddyfile", text)

    def test_compose_publishes_on_loopback_and_requires_a_password(self):
        compose = (CONTROL / "docker-compose.yml").read_text(encoding="utf-8")
        self.assertIn('"127.0.0.1:21114:21114"', compose)
        self.assertIn("POSTGRES_PASSWORD:?", compose)
        for needle in ("read_only: true", "cap_drop: [ALL]", "no-new-privileges"):
            self.assertIn(needle, compose)

    def test_proxy_examples_set_forwarded_for_from_the_peer(self):
        nginx = (CONTROL / "nginx.conf").read_text(encoding="utf-8")
        self.assertIn("proxy_set_header X-Forwarded-For $remote_addr;", nginx)

    def test_scripts_are_strict_posix_shell(self):
        for name in ("build-control-deb.sh", "build-control-rpm.sh"):
            body = (ROOT / name).read_text(encoding="utf-8")
            self.assertTrue(body.startswith("#!/bin/sh"), name)
            self.assertIn("set -eu", body)
