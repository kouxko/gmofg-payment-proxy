"""Contract tests for the reusable release-App external-package E2E runner."""

from __future__ import annotations

import json
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

from scripts import e2e_external_packages as e2e


class ExternalPackageWorkspaceTests(unittest.TestCase):
    def test_workspace_binds_each_listener_to_one_exact_external_package(self) -> None:
        workspace = e2e.build_workspace(revision=7)

        bindings = [
            listener["data_plane"]["settings"]["processing"]["settings"]["package"]
            for listener in workspace["listeners"]
        ]

        self.assertEqual(bindings, [e2e.DENO_PACKAGE, e2e.AU_EFTEX_PACKAGE])
        self.assertEqual(workspace["protocol_rules"], [])

    def test_install_preserves_non_e2e_workspaces(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "app.sqlite3"
            self._create_database(database)

            outcome = e2e.install_workspace(database, backup=False, require_app_stopped=False)

            with closing(sqlite3.connect(database)) as connection:
                workspaces = connection.execute(
                    "SELECT id FROM workspaces ORDER BY id"
                ).fetchall()
                selected = connection.execute(
                    "SELECT selected_id FROM workspace_state WHERE singleton_id = 1"
                ).fetchone()
            self.assertEqual(
                workspaces,
                [(e2e.WORKSPACE_ID,), ("original",)],
            )
            self.assertEqual(selected, (e2e.WORKSPACE_ID,))
            self.assertEqual(outcome.revision, 1)
            self.assertEqual(outcome.previous_selected_id, "original")

            e2e.restore_selected_workspace(
                database,
                outcome.previous_selected_id,
                require_app_stopped=False,
            )
            with closing(sqlite3.connect(database)) as connection:
                restored = connection.execute(
                    "SELECT selected_id FROM workspace_state WHERE singleton_id = 1"
                ).fetchone()
            self.assertEqual(restored, ("original",))

    def test_install_rejects_a_missing_exact_package_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "app.sqlite3"
            self._create_database(database, include_au_eftex=False)

            with self.assertRaisesRegex(e2e.AcceptanceError, "au-eftex@1.1.0"):
                e2e.install_workspace(database, backup=False, require_app_stopped=False)

    def test_install_rejects_a_disabled_package_without_changing_selection(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "app.sqlite3"
            self._create_database(database, au_eftex_enabled=False)

            with self.assertRaisesRegex(e2e.AcceptanceError, "disabled"):
                e2e.install_workspace(database, backup=False, require_app_stopped=False)

            with closing(sqlite3.connect(database)) as connection:
                selected = connection.execute(
                    "SELECT selected_id FROM workspace_state WHERE singleton_id = 1"
                ).fetchone()
                e2e_workspace = connection.execute(
                    "SELECT id FROM workspaces WHERE id = ?", (e2e.WORKSPACE_ID,)
                ).fetchone()
            self.assertEqual(selected, ("original",))
            self.assertIsNone(e2e_workspace)

    def test_restore_missing_workspace_preserves_current_selection(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "app.sqlite3"
            self._create_database(database)

            with self.assertRaisesRegex(e2e.AcceptanceError, "does not exist"):
                e2e.restore_selected_workspace(
                    database,
                    "missing",
                    require_app_stopped=False,
                )

            with closing(sqlite3.connect(database)) as connection:
                selected = connection.execute(
                    "SELECT selected_id FROM workspace_state WHERE singleton_id = 1"
                ).fetchone()
            self.assertEqual(selected, ("original",))

    def test_tampered_e2e_fixture_is_rejected_before_network_io(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "app.sqlite3"
            self._create_database(database)
            e2e.install_workspace(database, backup=False, require_app_stopped=False)
            with closing(sqlite3.connect(database)) as connection, connection:
                connection.execute(
                    "UPDATE workspaces SET json = ? WHERE id = ?",
                    ('{"tampered":true}', e2e.WORKSPACE_ID),
                )

            with self.assertRaisesRegex(e2e.AcceptanceError, "differs from the fixed fixture"):
                e2e.run_acceptance(database, "deno")

    def test_au_eftex_vectors_are_distinct_by_direction(self) -> None:
        self.assertNotEqual(e2e.AU_EFTEX_UPSTREAM_FRAME, e2e.AU_EFTEX_DOWNSTREAM_FRAME)
        self.assertTrue(e2e.AU_EFTEX_UPSTREAM_FRAME.startswith(b"T\xdf\x00"))
        self.assertTrue(e2e.AU_EFTEX_DOWNSTREAM_FRAME.startswith(b"T\xdf\x00"))

    def test_evidence_writer_emits_one_json_line_per_package(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "evidence.jsonl"

            e2e.write_evidence(
                target,
                {
                    "deno": {"request_bytes": 59, "response_bytes": 59},
                    "au_eftex": {"request_bytes": 71, "response_bytes": 63},
                },
            )

            rows = [json.loads(line) for line in target.read_text().splitlines()]
        self.assertEqual([row["test_id"] for row in rows], ["deno", "au_eftex"])
        self.assertTrue(all(row["status"] == "PASS" for row in rows))

    def test_evidence_writer_excludes_untrusted_result_fields(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "evidence.jsonl"

            e2e.write_evidence(
                target,
                {
                    "deno": {
                        "request_bytes": 59,
                        "response_bytes": 59,
                        "payload": "must-not-be-exported",
                    }
                },
            )

            text = target.read_text(encoding="utf-8")
        self.assertNotIn("payload", text)
        self.assertNotIn("must-not-be-exported", text)

    def test_evidence_writer_uses_stable_package_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "evidence.jsonl"

            e2e.write_evidence(
                target,
                {
                    "au_eftex": {"request_bytes": 71, "response_bytes": 63},
                    "deno": {"request_bytes": 59, "response_bytes": 59},
                },
            )

            rows = [json.loads(line) for line in target.read_text().splitlines()]
        self.assertEqual([row["test_id"] for row in rows], ["deno", "au_eftex"])

    def test_evidence_writer_rejects_negative_byte_counts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "evidence.jsonl"

            with self.assertRaisesRegex(e2e.AcceptanceError, "byte counts"):
                e2e.write_evidence(
                    target,
                    {"deno": {"request_bytes": -1, "response_bytes": 59}},
                )

    @staticmethod
    def _create_database(
        database: Path,
        *,
        include_au_eftex: bool = True,
        au_eftex_enabled: bool = True,
    ) -> None:
        with closing(sqlite3.connect(database)) as connection, connection:
            connection.executescript(
                """
                CREATE TABLE application_schema(singleton_id INTEGER PRIMARY KEY, version INTEGER);
                INSERT INTO application_schema VALUES (1, 19);
                CREATE TABLE workspaces(
                    id TEXT PRIMARY KEY, revision INTEGER NOT NULL,
                    json TEXT NOT NULL, updated_at TEXT NOT NULL
                );
                CREATE TABLE workspace_state(singleton_id INTEGER PRIMARY KEY, selected_id TEXT);
                CREATE TABLE external_protocol_packages(
                    package_id TEXT NOT NULL, version TEXT NOT NULL, enabled INTEGER NOT NULL,
                    PRIMARY KEY(package_id, version)
                );
                INSERT INTO workspaces VALUES ('original', 1, '{}', '2026-08-24T00:00:00Z');
                INSERT INTO workspace_state VALUES (1, 'original');
                INSERT INTO external_protocol_packages VALUES ('iso8583-deno-ascii', '1.0.0', 1);
                """
            )
            if include_au_eftex:
                connection.execute(
                    "INSERT INTO external_protocol_packages VALUES (?, ?, ?)",
                    (
                        e2e.AU_EFTEX_PACKAGE["id"],
                        e2e.AU_EFTEX_PACKAGE["version"],
                        int(au_eftex_enabled),
                    ),
                )


if __name__ == "__main__":
    unittest.main()
