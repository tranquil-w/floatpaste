#!/usr/bin/env python3
"""Unit tests for fp_verify.py (cold-start isolated verification harness)."""
from __future__ import annotations

import argparse
import io
import json
import os
import shutil
import sqlite3
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import zlib
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import fp_verify  # noqa: E402


def _create_mock_v11_db(db_path: Path) -> None:
    conn = sqlite3.connect(db_path)
    try:
        conn.executescript(
            """
            CREATE TABLE clip_items (
              id TEXT PRIMARY KEY,
              type TEXT NOT NULL,
              full_text TEXT NOT NULL,
              preview_text TEXT NOT NULL,
              search_text TEXT NOT NULL,
              source_app TEXT NULL,
              is_favorited INTEGER NOT NULL DEFAULT 0,
              hash TEXT NOT NULL,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL,
              last_used_at INTEGER NULL,
              deleted_at INTEGER NULL,
              image_path TEXT NULL,
              image_width INTEGER NULL,
              image_height INTEGER NULL,
              image_format TEXT NULL,
              file_size INTEGER NULL,
              file_paths TEXT NOT NULL DEFAULT '[]',
              file_count INTEGER NOT NULL DEFAULT 0,
              total_size INTEGER NULL,
              directory_count INTEGER NOT NULL DEFAULT 0,
              text_kind TEXT NOT NULL DEFAULT 'plain'
            );
            CREATE TABLE tags (
              name TEXT PRIMARY KEY COLLATE NOCASE,
              created_at INTEGER NOT NULL
            );
            CREATE TABLE clip_item_tags (
              item_id TEXT NOT NULL,
              tag_name TEXT NOT NULL,
              PRIMARY KEY (item_id, tag_name)
            );
            CREATE TABLE settings (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            CREATE TABLE excluded_apps (
              executable_name TEXT PRIMARY KEY
            );
            """
        )
        conn.execute(
            """
            INSERT INTO clip_items(
              id, type, full_text, preview_text, search_text, source_app,
              is_favorited, hash, created_at, updated_at
            ) VALUES ('item-1', 'text', 'Hello FloatPaste', 'Hello FloatPaste',
                      'hello floatpaste', 'WindowsTerminal.exe', 1, 'h1', 100, 100)
            """
        )
        conn.execute(
            """
            INSERT INTO clip_items(
              id, type, full_text, preview_text, search_text, source_app,
              is_favorited, hash, created_at, updated_at, deleted_at
            ) VALUES ('item-2', 'text', 'Deleted item', 'Deleted item',
                      'deleted item', NULL, 0, 'h2', 90, 90, 110)
            """
        )
        conn.execute("INSERT INTO tags(name, created_at) VALUES ('work', 100)")
        conn.execute("INSERT INTO clip_item_tags(item_id, tag_name) VALUES ('item-1', 'work')")
        conn.execute("INSERT INTO excluded_apps(executable_name) VALUES ('KeePassXC.exe')")
        conn.execute(
            "INSERT INTO settings(key, value) VALUES ('user_settings', ?)",
            (
                json.dumps(
                    {
                        "pauseMonitoring": True,
                        "historyLimit": 100,
                        "pickerPositionMode": "lastPosition",
                    }
                ),
            ),
        )
        conn.execute(
            "INSERT INTO settings(key, value) VALUES ('picker_last_position', ?)",
            (json.dumps({"x": 320, "y": 180, "width": 360, "height": 420}),),
        )
        conn.commit()
    finally:
        conn.close()


class FpVerifyTests(unittest.TestCase):
    def test_sanitize_instance_id_and_event_names(self) -> None:
        self.assertEqual(fp_verify.sanitize_instance_id("vf_2026.11:22!"), "vf-2026-11-22")
        with self.assertRaises(fp_verify.VerifyError):
            fp_verify.sanitize_instance_id("   ...  ")

        self.assertEqual(
            fp_verify.event_name_for("wake", "vf-1"),
            r"Local\FloatPaste.Wake.vf-1",
        )
        self.assertEqual(
            fp_verify.event_name_for("exit", "vf-1"),
            r"Local\FloatPaste.TestExit.vf-1",
        )
        self.assertEqual(
            fp_verify.install_shutdown_event_name(r"D:\Repos\FloatPaste\Target\Release\FloatPaste.exe"),
            "Local\\FloatPaste.InstallShutdown.d:/repos/floatpaste/target/release/floatpaste.exe",
        )

    def test_classify_window_role_and_onscreen_semantics(self) -> None:
        self.assertEqual(fp_verify.classify_window_role("FloatPaste · 速贴"), "picker")
        self.assertEqual(fp_verify.classify_window_role("FloatPaste · 搜索"), "search")
        self.assertEqual(fp_verify.classify_window_role("FloatPaste · 悬浮气泡"), "tooltip")
        self.assertEqual(fp_verify.classify_window_role("FloatPaste · 托盘菜单"), "tray")
        self.assertEqual(fp_verify.classify_window_role("Unknown"), "other")

        self.assertTrue(fp_verify.is_window_onscreen(visible=True, cloaked=0, left=100, width=300, height=400))
        # Offscreen parked picker/search stays IsWindowVisible=true at -32000.
        self.assertFalse(fp_verify.is_window_onscreen(visible=True, cloaked=0, left=-32000, width=300, height=400))
        # Cloaked during pre-reveal assembly must count as not onscreen.
        self.assertFalse(fp_verify.is_window_onscreen(visible=True, cloaked=1, left=100, width=300, height=400))
        self.assertFalse(fp_verify.is_window_onscreen(visible=False, cloaked=0, left=100, width=300, height=400))
        self.assertFalse(fp_verify.is_window_onscreen(visible=True, cloaked=0, left=100, width=0, height=400))

    def test_encode_png_rgb_and_upscale_rgb(self) -> None:
        # 2x1 RGB pixels: red and blue
        rgb = bytes([255, 0, 0, 0, 0, 255])
        scaled, w, h = fp_verify.upscale_rgb(rgb, 2, 1, 2)
        self.assertEqual((w, h), (4, 2))
        png = fp_verify.encode_png_rgb(scaled, w, h)
        self.assertTrue(png.startswith(b"\x89PNG\r\n\x1a\n"))

        # Parse IHDR + IDAT chunks to confirm valid dimensions and decompressed scanlines.
        ihdr_len = struct.unpack(">I", png[8:12])[0]
        self.assertEqual(png[12:16], b"IHDR")
        self.assertEqual(ihdr_len, 13)
        pw, ph, depth, ctype = struct.unpack(">IIBB", png[16:26])
        self.assertEqual((pw, ph, depth, ctype), (4, 2, 8, 2))

        idat_start = 8 + 4 + 4 + 13 + 4
        idat_len = struct.unpack(">I", png[idat_start : idat_start + 4])[0]
        self.assertEqual(png[idat_start + 4 : idat_start + 8], b"IDAT")
        compressed = png[idat_start + 8 : idat_start + 8 + idat_len]
        raw = zlib.decompress(compressed)
        expected_row = b"\x00" + bytes([255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255])
        self.assertEqual(raw, expected_row * 2)

    def test_readonly_db_queries_and_write_rejection(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "floatpaste.db"
            _create_mock_v11_db(db_path)

            summary = fp_verify.query_db(db_path, "summary")
            self.assertEqual(summary["active_items"], 1)
            self.assertEqual(summary["deleted_items"], 1)
            self.assertEqual(summary["favorited_active_items"], 1)
            self.assertEqual(summary["tags"], 1)
            self.assertEqual(summary["excluded_apps"], 1)
            self.assertTrue(summary["pause_monitoring"])
            self.assertEqual(summary["picker_position_mode"], "lastPosition")
            self.assertEqual(summary["picker_last_position"]["x"], 320)

            items = fp_verify.query_db(db_path, "items")
            self.assertEqual(len(items["items"]), 2)
            self.assertEqual(items["items"][0]["tags"], "work")

            settings = fp_verify.query_db(db_path, "settings")
            self.assertEqual(settings["settings"]["user_settings"]["historyLimit"], 100)

            excluded = fp_verify.query_db(db_path, "excluded")
            self.assertEqual(excluded["excluded_apps"], ["KeePassXC.exe"])

            ro_res = fp_verify.query_db(
                db_path, "sql", sql="SELECT id, is_favorited FROM clip_items WHERE deleted_at IS NULL;"
            )
            self.assertEqual(ro_res["rows"], [{"id": "item-1", "is_favorited": 1}])

            for bad_sql in (
                "DELETE FROM clip_items",
                "UPDATE clip_items SET is_favorited = 0",
                "INSERT INTO tags(name, created_at) VALUES('x', 1)",
                "SELECT 1; DELETE FROM tags",
                "/* sneaky */ DELETE FROM tags",
                "PRAGMA journal_mode = DELETE",
            ):
                with self.assertRaises(fp_verify.VerifyError, msg=bad_sql):
                    fp_verify.query_db(db_path, "sql", sql=bad_sql)

            # Confirm DB remains untouched.
            self.assertEqual(fp_verify.query_db(db_path, "summary")["active_items"], 1)

    def test_ensure_under_rejects_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp) / ".artifacts" / "verify-floatpaste"
            base.mkdir(parents=True)
            inside = fp_verify.ensure_under(base / "run1" / "evidence" / "shot.png", base)
            self.assertEqual(inside.name, "shot.png")

            with self.assertRaises(fp_verify.VerifyError):
                fp_verify.ensure_under(base / ".." / "escape.png", base)

    def test_cleanup_removes_runtime_and_preserves_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            run_id = "20261122-171500-abc123"
            _, runtime_dir, evidence_dir = fp_verify.run_dirs(repo, run_id)
            (runtime_dir / "appdata" / "floatpaste").mkdir(parents=True)
            (runtime_dir / "appdata" / "floatpaste" / "floatpaste.db").write_text("db", encoding="utf-8")
            evidence_dir.mkdir(parents=True)
            shot = evidence_dir / "quick-paste" / "picker.png"
            shot.parent.mkdir(parents=True)
            shot.write_bytes(b"\x89PNGmock")
            manifest = evidence_dir / "manifest.jsonl"
            manifest.write_text('{"feature":"quick-paste"}\n', encoding="utf-8")

            base = fp_verify.artifacts_base(repo)
            (base / "latest.txt").write_text(run_id, encoding="utf-8")
            session = {
                "run_id": run_id,
                "instance_id": f"vf-{run_id}",
                "pid": 999999,
                "exe": str(repo / "target" / "debug" / "floatpaste.exe"),
                "appdata_dir": str(runtime_dir / "appdata"),
                "runtime_dir": str(runtime_dir),
                "evidence_dir": str(evidence_dir),
            }
            (runtime_dir / "session.json").write_text(json.dumps(session), encoding="utf-8")

            res = fp_verify.cleanup_run_artifacts(
                repo,
                session,
                stop_process_cb=lambda pid, exe, iid, timeout: (True, False),
            )
            self.assertTrue(res["ok"])
            self.assertTrue(res["runtime_removed"])
            self.assertFalse(runtime_dir.exists())
            self.assertTrue(res["evidence_preserved"])
            self.assertTrue(shot.is_file())
            self.assertTrue(manifest.is_file())
            self.assertFalse((base / "latest.txt").exists())
            paths = {f["path"] for f in res["evidence_files"]}
            self.assertEqual(paths, {"manifest.jsonl", "quick-paste/picker.png"})

    def test_parse_cmdline_args(self) -> None:
        self.assertEqual(
            fp_verify.parse_cmdline_args(r'"C:\Program Files\FloatPaste\floatpaste.exe" --silent --flag=1'),
            ["--silent", "--flag=1"],
        )
        self.assertEqual(
            fp_verify.parse_cmdline_args(r"D:\repos\floatpaste\target\release\floatpaste.exe"),
            [],
        )
        self.assertEqual(fp_verify.parse_cmdline_args(None), [])

    def test_choose_capture_method_and_pixel_stats(self) -> None:
        self.assertEqual(
            fp_verify.choose_capture_method("auto", has_hwnd=True, same_as_input_desktop=True),
            "screen_bitblt",
        )
        self.assertEqual(
            fp_verify.choose_capture_method("auto", has_hwnd=True, same_as_input_desktop=False),
            "print_window",
        )
        self.assertEqual(
            fp_verify.choose_capture_method("auto", has_hwnd=False, same_as_input_desktop=False),
            "screen_bitblt",
        )
        self.assertEqual(
            fp_verify.choose_capture_method("screen", has_hwnd=True, same_as_input_desktop=False),
            "screen_bitblt",
        )
        self.assertEqual(
            fp_verify.choose_capture_method("window", has_hwnd=True, same_as_input_desktop=True),
            "print_window",
        )
        with self.assertRaises(fp_verify.VerifyError):
            fp_verify.choose_capture_method("window", has_hwnd=False, same_as_input_desktop=False)

        black = fp_verify.rgb_pixel_stats(bytes([0] * 12))
        self.assertTrue(black["all_black"])
        self.assertEqual(black["non_black_sample_ratio"], 0.0)
        self.assertEqual(black["unique_sample_colors"], 1)

        colored = fp_verify.rgb_pixel_stats(bytes([0, 0, 0, 255, 128, 64, 10, 20, 30]))
        self.assertFalse(colored["all_black"])
        self.assertGreater(colored["non_black_sample_ratio"], 0.5)
        self.assertEqual(colored["unique_sample_colors"], 3)

    def test_is_business_window_and_inspect_logs_classification(self) -> None:
        self.assertFalse(fp_verify.is_business_window("", "Winit Thread Event Target"))
        self.assertFalse(fp_verify.is_business_window("", "UAC_InputIndicatorOverlayWnd"))
        self.assertFalse(fp_verify.is_business_window("", "UAC Input Indicator"))
        self.assertTrue(fp_verify.is_business_window("FloatPaste · 速贴", "Window Class"))
        self.assertTrue(fp_verify.is_business_window("", "Window Class"))

        with tempfile.TemporaryDirectory() as tmp:
            appdata = Path(tmp)
            logs_dir = appdata / "floatpaste" / "logs"
            logs_dir.mkdir(parents=True)
            (logs_dir / "floatpaste-native.log.2026-11-22").write_text(
                "\n".join(
                    [
                        "2026-11-22T09:00:00Z  INFO floatpaste::system: 应用启动诊断 version=\"0.11.5\"",
                        "2026-11-22T09:00:01Z ERROR floatpaste_core::platform::windows::hotkey: 注册全局快捷键 id=1 失败: 热键已注册。 (0x80070581)",
                        "2026-11-22T09:00:02Z ERROR floatpaste::other: unexpected failure",
                    ]
                ),
                encoding="utf-8",
            )
            strict = fp_verify.inspect_logs(appdata, allow_hotkey_conflict=False)
            self.assertTrue(strict["startup_diagnostic_found"])
            self.assertEqual(len(strict["error_lines"]), 2)

            with_daily = fp_verify.inspect_logs(appdata, allow_hotkey_conflict=True)
            self.assertEqual(len(with_daily["hotkey_conflict_lines"]), 1)
            self.assertEqual(len(with_daily["error_lines"]), 1)
            self.assertIn("unexpected failure", with_daily["error_lines"][0])

    def test_content_window_role_classification_and_seed_last_position_db(self) -> None:
        self.assertEqual(fp_verify.classify_window_role("FloatPaste 托盘", "FloatPasteTrayWnd", 136), "tray-host")
        self.assertEqual(fp_verify.classify_window_role("floatpaste", "Window Class", 920), "settings")
        self.assertEqual(fp_verify.classify_window_role("floatpaste", "Window Class", 800), "editor")
        self.assertEqual(fp_verify.parse_xy_point("80,80"), (80, 80))
        with self.assertRaises(fp_verify.VerifyError):
            fp_verify.parse_xy_point("bad")

        with tempfile.TemporaryDirectory() as tmp:
            appdata = Path(tmp)
            db_path = fp_verify.seed_last_position_db(appdata, 80, 80)
            self.assertTrue(db_path.is_file())
            conn = sqlite3.connect(db_path)
            try:
                rows = dict(conn.execute("SELECT key, value FROM settings").fetchall())
            finally:
                conn.close()
            self.assertEqual(json.loads(rows["user_settings"])["pickerPositionMode"], "lastPosition")
            self.assertEqual(
                json.loads(rows["picker_last_position"]),
                {"x": 80, "y": 80, "width": 420, "height": 600},
            )

    def test_select_daily_instances_isolates_test_sessions(self) -> None:
        procs = [
            {"pid": 111, "exe": r"D:\repos\floatpaste\target\debug\floatpaste.exe", "cmdline": "debug"},
            {"pid": 222, "exe": r"D:\repos\floatpaste\target\release\floatpaste.exe", "cmdline": "release"},
        ]
        # No daily mutex -> never touch isolated instance.
        selected, err = fp_verify.select_daily_instances(
            procs[:1], mutex_active=False, isolated_pids=set()
        )
        self.assertEqual((selected, err), ([], None))

        # Both exist, PID 111 belongs to tracked isolated session -> only PID 222 selected.
        selected, err = fp_verify.select_daily_instances(
            procs, mutex_active=True, isolated_pids={111}
        )
        self.assertIsNone(err)
        self.assertEqual([p["pid"] for p in selected], [222])

        # Ambiguous candidates without --pid -> fail safe, unless --pid disambiguates.
        selected, err = fp_verify.select_daily_instances(
            procs, mutex_active=True, isolated_pids=set()
        )
        self.assertEqual(selected, [])
        self.assertIn("Multiple candidate", err or "")
        selected, err = fp_verify.select_daily_instances(
            procs, mutex_active=True, isolated_pids=set(), target_pid=222
        )
        self.assertIsNone(err)
        self.assertEqual([p["pid"] for p in selected], [222])

        # cmd_daily("pause") when only isolated process runs and mutex is inactive: zero signals.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            signaled: list[str] = []
            with (
                patch.object(fp_verify, "list_floatpaste_processes", lambda: procs[:1]),
                patch.object(fp_verify, "win32_mutex_exists", lambda _name: False),
                patch.object(fp_verify, "win32_signal_event", signaled.append),
            ):
                rc, res = fp_verify.cmd_daily(
                    argparse.Namespace(action="pause", timeout=1), root
                )
            self.assertEqual(rc, 0)
            self.assertFalse(res["was_running"])
            self.assertEqual(signaled, [])

    def test_readonly_sqlite_with_hash_and_percent_in_path(self) -> None:
        for prefix in ("fp_#hash_", "fp_%20_"):
            with tempfile.TemporaryDirectory(prefix=prefix) as tmp:
                parent = Path(tmp).parent
                sentinel_before = set(parent.iterdir())
                db = Path(tmp) / "floatpaste.db"
                _create_mock_v11_db(db)
                res = fp_verify.query_db(db, "sql", sql="SELECT id FROM clip_items WHERE deleted_at IS NULL")
                self.assertEqual(res["rows"], [{"id": "item-1"}])
                with self.assertRaises(sqlite3.OperationalError):
                    conn = fp_verify._open_ro_sqlite(db)
                    try:
                        conn.execute("DELETE FROM clip_items")
                    finally:
                        conn.close()
                self.assertEqual(set(parent.iterdir()), sentinel_before)

    def test_cleanup_rejects_corrupted_runtime_dir_pointing_to_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            run_id = "20261122-182000-deadbf"
            _, runtime_dir, evidence_dir = fp_verify.run_dirs(root, run_id)
            runtime_dir.mkdir(parents=True)
            evidence_dir.mkdir(parents=True)
            session = {
                "run_id": run_id,
                "instance_id": f"vf-{run_id}",
                "pid": 123,
                "exe": str(root / "target" / "debug" / "floatpaste.exe"),
                "runtime_dir": str(evidence_dir),
                "evidence_dir": str(evidence_dir),
            }
            with patch.object(shutil, "rmtree") as rm_mock:
                with self.assertRaises(fp_verify.VerifyError):
                    fp_verify.cleanup_run_artifacts(
                        root, session, stop_process_cb=lambda *_a: (True, False)
                    )
            rm_mock.assert_not_called()

            # Also verify missing evidence_dir after cleanup raises VerifyError.
            shutil.rmtree(evidence_dir)
            session["runtime_dir"] = str(runtime_dir)
            with self.assertRaises(fp_verify.VerifyError):
                fp_verify.cleanup_run_artifacts(
                    root, session, stop_process_cb=lambda *_a: (True, False)
                )

    def test_is_window_parked_rejects_onscreen_hidden_overlay(self) -> None:
        hidden_onscreen_search = {
            "role": "search",
            "onscreen": False,
            "visible": False,
            "rect": {"left": 100, "top": 100, "width": 800, "height": 600},
        }
        self.assertFalse(fp_verify.is_window_parked(hidden_onscreen_search))
        self.assertTrue(
            fp_verify.is_window_parked(
                {
                    "role": "search",
                    "onscreen": False,
                    "visible": True,
                    "rect": {"left": -32000, "top": -32000, "width": 780, "height": 181},
                }
            )
        )
        self.assertTrue(
            fp_verify.is_window_parked(
                {
                    "role": "editor",
                    "onscreen": False,
                    "visible": False,
                    "rect": {"left": 100, "top": 100, "width": 800, "height": 600},
                }
            )
        )
        with (
            patch.object(fp_verify, "load_session", lambda *_a: {"launch_desktop": "Default"}),
            patch.object(fp_verify, "_verified_session_pid", lambda _s: (123, "synthetic.exe", "vf-r")),
            patch.object(fp_verify, "win32_desktop_info", lambda: {"thread_desktop": "Default"}),
            patch.object(fp_verify, "win32_enum_windows_for_pid", lambda _pid: [hidden_onscreen_search]),
        ):
            args = fp_verify.build_parser().parse_args(
                ["windows", "--wait-role", "search", "--expect-state", "parked", "--timeout", "0"]
            )
            rc, res = fp_verify.cmd_windows(args, Path("."))
            self.assertEqual(rc, 1)
            self.assertFalse(res["ok"])

    def test_sql_mode_serializes_blob_cells_to_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db = Path(tmp) / "blob.db"
            conn = sqlite3.connect(db)
            conn.executescript(
                "CREATE TABLE clip_text_formats(item_id TEXT, html_bytes BLOB);"
                "INSERT INTO clip_text_formats VALUES ('1', X'00ff41');"
            )
            conn.commit()
            conn.close()

            buf = io.StringIO()
            with (
                patch.object(fp_verify, "load_session", lambda *_a: {"appdata_dir": "x", "run_id": "r"}),
                patch.object(fp_verify, "_open_ro_sqlite", lambda _p: sqlite3.connect(db)),
                patch("sys.stdout", buf),
            ):
                rc = fp_verify.main(["db", "--mode", "sql", "--sql", "SELECT html_bytes AS payload FROM clip_text_formats"])
            self.assertEqual(rc, 0)
            payload = json.loads(buf.getvalue())
            self.assertEqual(
                payload["rows"],
                [{"payload": {"type": "blob", "bytes": 3, "hex": "00ff41"}}],
            )

    def test_parse_cmdline_args_windows_escaping_roundtrip(self) -> None:
        orig_args = ["synthetic.exe", "--label", 'say "hello"', ""]
        cmd = subprocess.list2cmdline(orig_args)
        self.assertEqual(fp_verify.parse_cmdline_args(cmd), orig_args[1:])
        self.assertEqual(
            fp_verify.parse_cmdline_args(r'"C:\dir with space\\" --flag "a\\\"b"'),
            ["--flag", r'a\"b'],
        )

    def test_check_binary_status_detects_stale_sql_migration(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "target" / "debug").mkdir(parents=True)
            exe = root / "target" / "debug" / "floatpaste.exe"
            exe.write_bytes(b"MZ")
            mig_dir = root / "crates" / "floatpaste-core" / "migrations"
            mig_dir.mkdir(parents=True)
            sql_file = mig_dir / "0001_init.sql"
            sql_file.write_text("CREATE TABLE t(x INT);", encoding="utf-8")
            # Deterministic mtime bump without sleeping.
            exe_mtime = exe.stat().st_mtime
            os.utime(sql_file, (exe_mtime + 10.0, exe_mtime + 10.0))

            status = fp_verify.check_binary_status(root, exe)
            self.assertTrue(status["build_stale"])
            self.assertEqual(Path(status["newest_source_path"]).name, "0001_init.sql")


if __name__ == "__main__":
    unittest.main()
