# SPDX-License-Identifier: AGPL-3.0-only
import tempfile
import unittest
from pathlib import Path

from tools import check_client_network as check


class NetworkCheckTests(unittest.TestCase):
    def write(self, data):
        handle = tempfile.NamedTemporaryFile(delete=False)
        handle.write(data)
        handle.close()
        self.addCleanup(Path(handle.name).unlink)
        return Path(handle.name)

    def test_clean_file(self):
        self.assertEqual(check.scan(self.write(b"docs link https://rustdesk.com/docs ok")), [])

    def test_finds_utf8_and_utf16(self):
        self.assertEqual(check.scan(self.write(b"xx api.rustdesk.com yy")), ["api.rustdesk.com"])
        wide = "admin.rustdesk.com".encode("utf-16-le")
        self.assertEqual(check.scan(self.write(b"\0\0" + wide + b"\0")), ["admin.rustdesk.com"])

    def test_finds_string_across_chunk_boundary(self):
        data = b"a" * (check.CHUNK - 5) + b"rs-ny.rustdesk.com" + b"b" * 10
        self.assertEqual(check.scan(self.write(data)), ["rs-ny.rustdesk.com"])

    def test_exit_codes(self):
        clean = self.write(b"nothing")
        dirty = self.write(b"api.rustdesk.com")
        self.assertEqual(check.main(["x", str(clean)]), 0)
        self.assertEqual(check.main(["x", str(clean), str(dirty)]), 1)
        self.assertEqual(check.main(["x", "no-such-file"]), 2)
        self.assertEqual(check.main(["x"]), 2)


if __name__ == "__main__":
    unittest.main()
