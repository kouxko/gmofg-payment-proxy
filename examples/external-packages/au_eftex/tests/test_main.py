from __future__ import annotations

import os
import stat
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from au_eftex.main import _allow_insecure_remote_ws, _load_secret, main


class SecretConfigurationTests(unittest.TestCase):
    def test_ctrl_c_after_async_cleanup_is_a_clean_operator_shutdown(self) -> None:
        def interrupt(coroutine: object) -> None:
            getattr(coroutine, "close")()
            raise KeyboardInterrupt

        with patch("au_eftex.main.asyncio.run", side_effect=interrupt):
            main()

    def test_remote_plaintext_ws_requires_an_exact_explicit_opt_in(self) -> None:
        self.assertFalse(_allow_insecure_remote_ws({}))
        self.assertFalse(_allow_insecure_remote_ws({"AU_EFTEX_ALLOW_INSECURE_REMOTE_WS": "0"}))
        self.assertTrue(_allow_insecure_remote_ws({"AU_EFTEX_ALLOW_INSECURE_REMOTE_WS": "1"}))
        with self.assertRaisesRegex(RuntimeError, "must be 0 or 1"):
            _allow_insecure_remote_ws({"AU_EFTEX_ALLOW_INSECURE_REMOTE_WS": "true"})

    def test_loads_a_secret_from_a_owner_only_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "bdk.hex")
            path.write_text("0123456789ABCDEFFEDCBA9876543210\n", encoding="ascii")
            path.chmod(stat.S_IRUSR | stat.S_IWUSR)

            secret = _load_secret(
                "AU_EFTEX_BDK",
                16,
                {"AU_EFTEX_BDK_FILE": os.fspath(path)},
            )

            self.assertEqual(secret, bytes.fromhex("0123456789ABCDEFFEDCBA9876543210"))

    def test_rejects_a_secret_file_readable_by_group_or_other_users(self) -> None:
        if os.name == "nt":
            self.skipTest("POSIX mode bits are not authoritative on Windows")
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "bdk.hex")
            path.write_text("0123456789ABCDEFFEDCBA9876543210", encoding="ascii")
            path.chmod(stat.S_IRUSR | stat.S_IRGRP)

            with self.assertRaisesRegex(RuntimeError, "owner-only"):
                _load_secret(
                    "AU_EFTEX_BDK",
                    16,
                    {"AU_EFTEX_BDK_FILE": os.fspath(path)},
                )

    def test_rejects_ambiguous_file_and_environment_sources(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "exactly one"):
            _load_secret(
                "AU_EFTEX_BDK",
                16,
                {
                    "AU_EFTEX_BDK_FILE": "/not/read",
                    "AU_EFTEX_BDK_HEX": "0123456789ABCDEFFEDCBA9876543210",
                },
            )

    def test_error_does_not_repeat_the_secret_value(self) -> None:
        supplied = "not-a-valid-secret"

        with self.assertRaises(RuntimeError) as raised:
            _load_secret("AU_EFTEX_BDK", 16, {"AU_EFTEX_BDK_HEX": supplied})

        self.assertNotIn(supplied, str(raised.exception))


if __name__ == "__main__":
    unittest.main()
