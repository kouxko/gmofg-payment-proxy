"""Regression tests for the local HTTP/Socket proxy acceptance harness."""

from __future__ import annotations

import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from scripts import e2e_proxy_rules as e2e


class Iso8583FixtureTests(unittest.TestCase):
    def test_amount_rewriter_preserves_frame_shape_and_updates_known_field(self) -> None:
        frame = bytes.fromhex(e2e.ISO8583_SAMPLE_HEX)

        parsed = e2e.parse_iso8583_sample(frame)
        rewritten = e2e.with_iso8583_amount(frame, 2222)

        self.assertEqual(parsed.message_type, "0200")
        self.assertEqual(parsed.amount, 1000)
        self.assertEqual(len(rewritten), len(frame))
        self.assertEqual(e2e.parse_iso8583_sample(rewritten).amount, 2222)

    def test_response_rewriter_changes_only_expected_iso_fields(self) -> None:
        request = bytes.fromhex(e2e.ISO8583_SAMPLE_HEX)

        response = e2e.with_iso8583_message_type(
            e2e.with_iso8583_amount(request, 2222),
            "0210",
        )

        parsed = e2e.parse_iso8583_sample(response)
        self.assertEqual(parsed.message_type, "0210")
        self.assertEqual(parsed.amount, 2222)
        self.assertEqual(response[:2], request[:2])


class WorkspaceFixtureTests(unittest.TestCase):
    def test_fixture_contains_http_scripted_socket_and_raw_transparent_socket(self) -> None:
        workspace = e2e.build_workspace(revision=7)

        self.assertEqual(workspace["revision"], 7)
        self.assertEqual(
            [listener["data_plane"]["kind"] for listener in workspace["listeners"]],
            ["http", "socket", "socket"],
        )
        raw = workspace["listeners"][2]["data_plane"]["settings"]
        self.assertEqual(raw["topology"]["settings"]["security"], {"mode": "transparent"})
        self.assertEqual(raw["processing"], {"mode": "direct"})
        self.assertEqual(len(workspace["rules"]), 2)
        self.assertEqual(
            [rule["stage"] for rule in workspace["protocol_rules"]],
            ["app_to_proxy", "proxy_to_upstream", "upstream_to_proxy", "proxy_to_app"],
        )

    def test_install_preserves_existing_workspace_and_selects_e2e_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "intercept-proxy.sqlite3"
            connection = sqlite3.connect(database)
            connection.executescript(
                """
                CREATE TABLE application_schema(singleton_id INTEGER PRIMARY KEY, version INTEGER NOT NULL);
                CREATE TABLE workspaces(id TEXT PRIMARY KEY, revision INTEGER NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL);
                CREATE TABLE workspace_state(singleton_id INTEGER PRIMARY KEY, selected_id TEXT NULL);
                CREATE TABLE protocol_packages(
                    package_id TEXT NOT NULL,
                    version TEXT NOT NULL,
                    enabled INTEGER NOT NULL,
                    validation_state TEXT NOT NULL,
                    PRIMARY KEY(package_id, version)
                );
                INSERT INTO application_schema VALUES (1, 19);
                INSERT INTO workspaces VALUES ('original', 1, '{}', '2026-08-22T00:00:00Z');
                INSERT INTO workspace_state VALUES (1, 'original');
                INSERT INTO protocol_packages VALUES ('iso8583-ascii-standard', '1.0.0', 1, 'valid');
                """
            )
            connection.commit()
            connection.close()

            outcome = e2e.install_workspace(database, backup=False)

            connection = sqlite3.connect(database)
            rows = connection.execute("SELECT id, revision, json FROM workspaces ORDER BY id").fetchall()
            selected = connection.execute(
                "SELECT selected_id FROM workspace_state WHERE singleton_id = 1"
            ).fetchone()[0]
            connection.close()
            self.assertEqual({row[0] for row in rows}, {"original", e2e.WORKSPACE_ID})
            self.assertEqual(selected, e2e.WORKSPACE_ID)
            fixture_row = next(row for row in rows if row[0] == e2e.WORKSPACE_ID)
            self.assertEqual(json.loads(fixture_row[2])["revision"], fixture_row[1])
            self.assertEqual(outcome.workspace_id, e2e.WORKSPACE_ID)

    def test_selected_fixture_allows_runtime_rule_counters_to_change(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "intercept-proxy.sqlite3"
            workspace = e2e.build_workspace(revision=9)
            workspace["rules"][0]["hit_count"] = 12
            workspace["rules"][0]["last_hit_at"] = "2026-08-24T12:00:00Z"
            self._write_selected_workspace(database, workspace)

            e2e._assert_fixture_selected(database)

    def test_selected_fixture_still_rejects_configuration_drift(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "intercept-proxy.sqlite3"
            workspace = e2e.build_workspace(revision=9)
            workspace["listeners"][0]["bind_port"] = 19_999
            self._write_selected_workspace(database, workspace)

            with self.assertRaisesRegex(e2e.AcceptanceError, "differs"):
                e2e._assert_fixture_selected(database)

    @staticmethod
    def _write_selected_workspace(database: Path, workspace: dict[str, object]) -> None:
        connection = sqlite3.connect(database)
        connection.executescript(
            """
            CREATE TABLE workspaces(id TEXT PRIMARY KEY, revision INTEGER NOT NULL, json TEXT NOT NULL);
            CREATE TABLE workspace_state(singleton_id INTEGER PRIMARY KEY, selected_id TEXT NULL);
            """
        )
        connection.execute(
            "INSERT INTO workspace_state(singleton_id, selected_id) VALUES (1, ?)",
            (e2e.WORKSPACE_ID,),
        )
        connection.execute(
            "INSERT INTO workspaces(id, revision, json) VALUES (?, ?, ?)",
            (
                e2e.WORKSPACE_ID,
                int(workspace["revision"]),
                json.dumps(workspace, ensure_ascii=False),
            ),
        )
        connection.commit()
        connection.close()


if __name__ == "__main__":
    unittest.main()
