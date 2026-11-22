#!/usr/bin/env python3
"""Cold-start isolated desktop verification harness for FloatPaste (Windows, stdlib-only).

Subcommands:
  doctor     Read-only environment, daily-instance, binary staleness, and session health check
  launch     Start an isolated FloatPaste instance under .artifacts/verify-floatpaste/<run-id>/runtime
  signal     Trigger wake (--via-second-launch optional) or named test events on the isolated instance
  windows    Inspect or wait for owned windows of the isolated instance (onscreen vs parked)
  foreground Inspect current OS foreground window and lock-screen state
  capture    Capture DPI-aware PNG screenshot of a window or screen rect into evidence/ + manifest.jsonl
  db         Read-only SQLite query against isolated floatpaste.db (summary/items/settings/excluded/sql)
  daily      Gracefully check, pause, or restore the user's daily FloatPaste instance
  cleanup    Gracefully exit tracked PID, remove runtime/ only, verify and list surviving evidence/
"""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
from datetime import datetime
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import struct
import subprocess
import sys
import time
import uuid
import zlib
from typing import Any

IS_WINDOWS = sys.platform == "win32"

OFFSCREEN_THRESHOLD = -30_000
DAILY_MUTEX_NAME = r"Local\FloatPaste.SingleInstance"
WAKE_EVENT_PREFIX = r"Local\FloatPaste.Wake"
INSTALL_SHUTDOWN_PREFIX = "Local\\FloatPaste.InstallShutdown."
TEST_EVENT_BASES: dict[str, str] = {
    "exit": r"Local\FloatPaste.TestExit",
    "toggle-monitoring": r"Local\FloatPaste.TestToggleMonitoring",
    "open-search": r"Local\FloatPaste.TestOpenSearch",
    "search-escape": r"Local\FloatPaste.TestSearchEscape",
    "picker-favorite": r"Local\FloatPaste.TestPickerFavorite",
    "picker-delete": r"Local\FloatPaste.TestPickerDelete",
    "search-delete": r"Local\FloatPaste.TestSearchDelete",
}

WINDOW_ROLE_BY_TITLE: dict[str, str] = {
    "FloatPaste · 速贴": "picker",
    "FloatPaste · 搜索": "search",
    "FloatPaste · 悬浮气泡": "tooltip",
    "FloatPaste · 托盘菜单": "tray",
    "FloatPaste · 编辑": "editor",
    "FloatPaste · 设置": "settings",
    "粘贴为文件": "text-file",
}

ISOLATED_ENV_CLEAR = (
    "FLOATPASTE_E2E_EXE",
    "FLOATPASTE_UI_SNAPSHOTS",
    "FLOATPASTE_SAVE_EDITOR_SNAPSHOTS",
    "FLOATPASTE_TEXT_FILE_SNAPSHOTS",
    "FLOATPASTE_RETENTION_FIRST_DELAY_MS",
    "FLOATPASTE_DIAGNOSTIC_LOG",
)

# Win32 constants
ERROR_FILE_NOT_FOUND = 2
SYNCHRONIZE = 0x0010_0000
EVENT_MODIFY_STATE = 0x0002
PROCESS_TERMINATE = 0x0001
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
STILL_ACTIVE = 259
GWL_EXSTYLE = -20
WS_EX_TOPMOST = 0x0000_0008
DWMWA_CLOAKED = 14
SM_XVIRTUALSCREEN = 76
SM_YVIRTUALSCREEN = 77
SM_CXVIRTUALSCREEN = 78
SM_CYVIRTUALSCREEN = 79
SRCCOPY_CAPTUREBLT = 0x00CC_0020 | 0x4000_0000
DIB_RGB_COLORS = 0
BI_RGB = 0
DESKTOP_READOBJECTS = 0x0001
UOI_NAME = 2
PW_RENDERFULLCONTENT = 2
CREATE_NEW_PROCESS_GROUP = 0x0000_0200
DETACHED_PROCESS = 0x0000_0008
CREATE_NO_WINDOW = 0x0800_0000


class VerifyError(RuntimeError):
    """Expected operational or validation error surfaced as JSON."""


def configure_stdio() -> None:
    for stream in (sys.stdout, sys.stderr):
        reconf = getattr(stream, "reconfigure", None)
        if reconf is not None:
            try:
                reconf(encoding="utf-8", errors="replace")
            except OSError:
                pass


def sanitize_instance_id(raw: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9-]+", "-", raw.strip()).strip("-")
    if not cleaned:
        raise VerifyError(f"Invalid instance id / run id after sanitization: {raw!r}")
    return cleaned


def event_name_for(action: str, instance_id: str) -> str:
    iid = sanitize_instance_id(instance_id)
    if action == "wake":
        return f"{WAKE_EVENT_PREFIX}.{iid}"
    if action in TEST_EVENT_BASES:
        return f"{TEST_EVENT_BASES[action]}.{iid}"
    raise VerifyError(f"Unknown signal action: {action!r}")


def mutex_name_for(instance_id: str | None = None) -> str:
    if instance_id is None:
        return DAILY_MUTEX_NAME
    return f"{DAILY_MUTEX_NAME}.{sanitize_instance_id(instance_id)}"


def install_shutdown_event_name(exe_path: str | Path) -> str:
    normalized = str(exe_path).strip().lower().replace("\\", "/")
    if not normalized:
        raise VerifyError("Executable path cannot be empty for InstallShutdown event")
    return f"{INSTALL_SHUTDOWN_PREFIX}{normalized}"


def classify_window_role(title: str, class_name: str = "", width: int = 0) -> str:
    stripped = title.strip()
    if stripped in WINDOW_ROLE_BY_TITLE:
        return WINDOW_ROLE_BY_TITLE[stripped]
    if stripped == "FloatPaste 托盘" or class_name == "FloatPasteTrayWnd":
        return "tray-host"
    if stripped.lower() == "floatpaste" and class_name == "Window Class":
        return "settings" if width >= 880 else "editor"
    return "other"


def parse_xy_point(raw: str) -> tuple[int, int]:
    parts = [p.strip() for p in raw.split(",")]
    if len(parts) != 2:
        raise VerifyError(f"--picker-pos must be x,y; got {raw!r}")
    try:
        return int(parts[0]), int(parts[1])
    except ValueError as exc:
        raise VerifyError(f"--picker-pos coordinates must be integers; got {raw!r}") from exc


def seed_last_position_db(
    appdata_dir: Path, x: int, y: int, width: int = 420, height: int = 600
) -> Path:
    fp_dir = appdata_dir / "floatpaste"
    fp_dir.mkdir(parents=True, exist_ok=True)
    db_path = fp_dir / "floatpaste.db"
    conn = sqlite3.connect(db_path)
    try:
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)"
        )
        conn.execute(
            "INSERT OR REPLACE INTO settings(key, value) VALUES (?, ?)",
            ("user_settings", json.dumps({"pickerPositionMode": "lastPosition"})),
        )
        conn.execute(
            "INSERT OR REPLACE INTO settings(key, value) VALUES (?, ?)",
            (
                "picker_last_position",
                json.dumps({"x": int(x), "y": int(y), "width": int(width), "height": int(height)}),
            ),
        )
        conn.commit()
    finally:
        conn.close()
    return db_path


SYSTEM_UTILITY_CLASSES = frozenset(
    {
        "Winit Thread Event Target",
        "UAC_InputIndicatorOverlayWnd",
        "UAC Input Indicator",
        "IME",
        "MSCTFIME UI",
        "tooltips_class32",
    }
)


def is_business_window(title: str, class_name: str) -> bool:
    if class_name in SYSTEM_UTILITY_CLASSES:
        return False
    stripped = title.strip()
    if stripped.startswith("FloatPaste") or stripped.lower() == "floatpaste" or stripped == "粘贴为文件":
        return True
    return class_name == "Window Class"


def is_window_onscreen(
    *, visible: bool, cloaked: int, left: int, width: int, height: int
) -> bool:
    return bool(visible) and cloaked == 0 and left > OFFSCREEN_THRESHOLD and width > 0 and height > 0


OFFSCREEN_PARKED_ROLES = frozenset({"picker", "search", "tooltip", "tray"})


def is_window_parked(win: dict[str, Any]) -> bool:
    """Verify parked state: overlay windows must sit offscreen (<= -30000), not SW_HIDE onscreen."""
    if win.get("onscreen"):
        return False
    if win.get("role") in OFFSCREEN_PARKED_ROLES:
        rect = win.get("rect") or {}
        return (
            int(rect.get("left", 0)) <= OFFSCREEN_THRESHOLD
            and int(rect.get("top", 0)) <= OFFSCREEN_THRESHOLD
        )
    return True


def ensure_under(path: Path, base: Path) -> Path:
    resolved_base = base.resolve()
    resolved_path = path.resolve()
    try:
        resolved_path.relative_to(resolved_base)
    except ValueError as exc:
        raise VerifyError(
            f"Refusing path outside allowed root {resolved_base}: {resolved_path}"
        ) from exc
    return resolved_path


def parse_cmdline_args(cmdline: str | None) -> list[str]:
    """Parse Windows CreateProcess command line into argv[1:] following MSVC/CommandLineToArgvW rules."""
    if not cmdline or not cmdline.strip():
        return []
    args: list[str] = []
    i = 0
    n = len(cmdline)
    while i < n:
        while i < n and cmdline[i] in (" ", "\t"):
            i += 1
        if i >= n:
            break
        cur: list[str] = []
        in_quotes = False
        while i < n:
            ch = cmdline[i]
            if ch == "\\":
                bs = 0
                while i < n and cmdline[i] == "\\":
                    bs += 1
                    i += 1
                if i < n and cmdline[i] == '"':
                    cur.append("\\" * (bs // 2))
                    if bs % 2 == 1:
                        cur.append('"')
                        i += 1
                else:
                    cur.append("\\" * bs)
                continue
            if ch == '"':
                if in_quotes and i + 1 < n and cmdline[i + 1] == '"':
                    cur.append('"')
                    i += 2
                    continue
                in_quotes = not in_quotes
                i += 1
                continue
            if ch in (" ", "\t") and not in_quotes:
                break
            cur.append(ch)
            i += 1
        args.append("".join(cur))
    return args[1:] if len(args) > 1 else []


def default_repo_root() -> Path:
    # <repo>/.agents/skills/verify-floatpaste/scripts/fp_verify.py -> parents[4]
    return Path(__file__).resolve().parents[4]


def artifacts_base(repo_root: Path) -> Path:
    return repo_root / ".artifacts" / "verify-floatpaste"


def run_dirs(repo_root: Path, run_id: str) -> tuple[Path, Path, Path]:
    rid = sanitize_instance_id(run_id)
    base = artifacts_base(repo_root)
    root = ensure_under(base / rid, base)
    return root, root / "runtime", root / "evidence"


def resolve_run_id(repo_root: Path, requested: str | None) -> str:
    if requested:
        return sanitize_instance_id(requested)
    latest = artifacts_base(repo_root) / "latest.txt"
    if not latest.is_file():
        raise VerifyError(
            f"No --session given and {latest} does not exist. Run `launch` first."
        )
    value = latest.read_text(encoding="utf-8-sig").strip()
    if not value:
        raise VerifyError(f"{latest} is empty. Run `launch` first.")
    return sanitize_instance_id(value)


def load_session(repo_root: Path, requested: str | None) -> dict[str, Any]:
    run_id = resolve_run_id(repo_root, requested)
    _, runtime_dir, _ = run_dirs(repo_root, run_id)
    session_file = runtime_dir / "session.json"
    if not session_file.is_file():
        raise VerifyError(f"Session metadata not found: {session_file}")
    try:
        data = json.loads(session_file.read_text(encoding="utf-8-sig"))
    except (OSError, json.JSONDecodeError) as exc:
        raise VerifyError(f"Failed to read {session_file}: {exc}") from exc
    if not isinstance(data, dict):
        raise VerifyError(f"Invalid session metadata object in {session_file}")
    return data


def same_path(a: str | Path, b: str | Path) -> bool:
    return os.path.normcase(os.path.abspath(str(a))) == os.path.normcase(os.path.abspath(str(b)))


# ---------------------------------------------------------------------------
# Pure-stdlib PNG encoder
# ---------------------------------------------------------------------------

def _png_chunk(tag: bytes, data: bytes) -> bytes:
    body = tag + data
    crc = zlib.crc32(body) & 0xFFFF_FFFF
    return struct.pack(">I", len(data)) + body + struct.pack(">I", crc)


def upscale_rgb(rgb: bytes | bytearray, width: int, height: int, zoom: int) -> tuple[bytes, int, int]:
    if zoom < 1:
        raise VerifyError(f"zoom must be >= 1, got {zoom}")
    if zoom == 1:
        return bytes(rgb), width, height
    new_w = width * zoom
    new_h = height * zoom
    row_len = width * 3
    out = bytearray()
    for y in range(height):
        row = rgb[y * row_len : (y + 1) * row_len]
        scaled_row = bytearray(new_w * 3)
        for z in range(zoom):
            scaled_row[z * 3 :: 3 * zoom] = row[0::3]
            scaled_row[z * 3 + 1 :: 3 * zoom] = row[1::3]
            scaled_row[z * 3 + 2 :: 3 * zoom] = row[2::3]
        for _ in range(zoom):
            out.extend(scaled_row)
    return bytes(out), new_w, new_h


def encode_png_rgb(rgb: bytes | bytearray, width: int, height: int) -> bytes:
    if width <= 0 or height <= 0:
        raise VerifyError(f"Invalid PNG dimensions: {width}x{height}")
    expected = width * height * 3
    if len(rgb) != expected:
        raise VerifyError(f"RGB buffer size mismatch: got {len(rgb)}, expected {expected}")
    row_len = width * 3
    raw = b"".join(b"\x00" + bytes(rgb[y * row_len : (y + 1) * row_len]) for y in range(height))
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    idat = zlib.compress(raw, level=6)
    return b"\x89PNG\r\n\x1a\n" + _png_chunk(b"IHDR", ihdr) + _png_chunk(b"IDAT", idat) + _png_chunk(b"IEND", b"")


def choose_capture_method(
    requested: str, *, has_hwnd: bool, same_as_input_desktop: bool
) -> str:
    if requested == "screen":
        return "screen_bitblt"
    if requested == "window":
        if not has_hwnd:
            raise VerifyError("--method window requires a target window role (--role), not --rect.")
        return "print_window"
    if requested == "auto":
        return "print_window" if (has_hwnd and not same_as_input_desktop) else "screen_bitblt"
    raise VerifyError(f"Unknown capture method: {requested!r}")


def rgb_pixel_stats(rgb: bytes | bytearray) -> dict[str, Any]:
    total_pixels = len(rgb) // 3
    if total_pixels == 0:
        return {
            "all_black": True,
            "sampled_pixels": 0,
            "non_black_sample_ratio": 0.0,
            "unique_sample_colors": 0,
        }
    all_black = not any(rgb)
    sample_target = min(total_pixels, 2048)
    step = max(1, total_pixels // sample_target)
    indices = range(0, total_pixels, step)
    non_black = 0
    colors: set[tuple[int, int, int]] = set()
    sampled = 0
    for idx in indices:
        off = idx * 3
        px = (rgb[off], rgb[off + 1], rgb[off + 2])
        sampled += 1
        colors.add(px)
        if px != (0, 0, 0):
            non_black += 1
    return {
        "all_black": all_black,
        "sampled_pixels": sampled,
        "non_black_sample_ratio": round(non_black / sampled, 4) if sampled else 0.0,
        "unique_sample_colors": len(colors),
    }


# ---------------------------------------------------------------------------
# Win32 ctypes bindings (lazy on Windows so tests can import on any OS)
# ---------------------------------------------------------------------------

class _RECT(ctypes.Structure):
    _fields_ = [
        ("left", wintypes.LONG),
        ("top", wintypes.LONG),
        ("right", wintypes.LONG),
        ("bottom", wintypes.LONG),
    ]


class _BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", wintypes.LONG),
        ("biHeight", wintypes.LONG),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", wintypes.LONG),
        ("biYPelsPerMeter", wintypes.LONG),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class _BITMAPINFO(ctypes.Structure):
    _fields_ = [
        ("bmiHeader", _BITMAPINFOHEADER),
        ("bmiColors", wintypes.DWORD * 3),
    ]


_WIN32_CACHE: dict[str, Any] = {}


def _win32() -> dict[str, Any]:
    if not IS_WINDOWS:
        raise VerifyError("Win32 runtime features require Windows.")
    if _WIN32_CACHE:
        return _WIN32_CACHE

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
    dwmapi = ctypes.WinDLL("dwmapi", use_last_error=True)

    # Enable per-monitor v2 DPI awareness so GetWindowRect and CopyScreen use physical pixels.
    set_ctx = getattr(user32, "SetProcessDpiAwarenessContext", None)
    ok_dpi = False
    if set_ctx is not None:
        set_ctx.argtypes = [ctypes.c_void_p]
        set_ctx.restype = wintypes.BOOL
        ok_dpi = bool(set_ctx(ctypes.c_void_p(-4)))
    if not ok_dpi:
        set_legacy = getattr(user32, "SetProcessDPIAware", None)
        if set_legacy is not None:
            set_legacy.argtypes = []
            set_legacy.restype = wintypes.BOOL
            set_legacy()

    # Kernel32 signatures
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CloseHandle.restype = wintypes.BOOL

    kernel32.OpenMutexW.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.LPCWSTR]
    kernel32.OpenMutexW.restype = wintypes.HANDLE

    kernel32.OpenEventW.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.LPCWSTR]
    kernel32.OpenEventW.restype = wintypes.HANDLE

    kernel32.SetEvent.argtypes = [wintypes.HANDLE]
    kernel32.SetEvent.restype = wintypes.BOOL

    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE

    kernel32.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    kernel32.GetExitCodeProcess.restype = wintypes.BOOL

    kernel32.QueryFullProcessImageNameW.argtypes = [
        wintypes.HANDLE,
        wintypes.DWORD,
        wintypes.LPWSTR,
        ctypes.POINTER(wintypes.DWORD),
    ]
    kernel32.QueryFullProcessImageNameW.restype = wintypes.BOOL

    kernel32.TerminateProcess.argtypes = [wintypes.HANDLE, wintypes.UINT]
    kernel32.TerminateProcess.restype = wintypes.BOOL

    kernel32.GetCurrentThreadId.argtypes = []
    kernel32.GetCurrentThreadId.restype = wintypes.DWORD

    # User32 signatures
    enum_cb = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.EnumWindows.argtypes = [enum_cb, wintypes.LPARAM]
    user32.EnumWindows.restype = wintypes.BOOL

    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.GetWindowThreadProcessId.restype = wintypes.DWORD

    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetWindowTextW.restype = ctypes.c_int

    user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetClassNameW.restype = ctypes.c_int

    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.IsWindowVisible.restype = wintypes.BOOL

    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(_RECT)]
    user32.GetWindowRect.restype = wintypes.BOOL

    user32.GetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int]
    user32.GetWindowLongPtrW.restype = ctypes.c_ssize_t

    user32.GetDpiForWindow.argtypes = [wintypes.HWND]
    user32.GetDpiForWindow.restype = wintypes.UINT

    user32.GetForegroundWindow.argtypes = []
    user32.GetForegroundWindow.restype = wintypes.HWND

    user32.GetSystemMetrics.argtypes = [ctypes.c_int]
    user32.GetSystemMetrics.restype = ctypes.c_int

    user32.OpenClipboard.argtypes = [wintypes.HWND]
    user32.OpenClipboard.restype = wintypes.BOOL
    user32.CloseClipboard.argtypes = []
    user32.CloseClipboard.restype = wintypes.BOOL

    user32.GetDC.argtypes = [wintypes.HWND]
    user32.GetDC.restype = wintypes.HDC
    user32.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
    user32.ReleaseDC.restype = ctypes.c_int

    user32.GetThreadDesktop.argtypes = [wintypes.DWORD]
    user32.GetThreadDesktop.restype = wintypes.HANDLE
    user32.OpenInputDesktop.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    user32.OpenInputDesktop.restype = wintypes.HANDLE
    user32.CloseDesktop.argtypes = [wintypes.HANDLE]
    user32.CloseDesktop.restype = wintypes.BOOL
    user32.GetUserObjectInformationW.argtypes = [
        wintypes.HANDLE,
        ctypes.c_int,
        ctypes.c_void_p,
        wintypes.DWORD,
        ctypes.POINTER(wintypes.DWORD),
    ]
    user32.GetUserObjectInformationW.restype = wintypes.BOOL
    user32.PrintWindow.argtypes = [wintypes.HWND, wintypes.HDC, wintypes.UINT]
    user32.PrintWindow.restype = wintypes.BOOL

    # GDI32 signatures
    gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]
    gdi32.CreateCompatibleDC.restype = wintypes.HDC
    gdi32.DeleteDC.argtypes = [wintypes.HDC]
    gdi32.DeleteDC.restype = wintypes.BOOL

    gdi32.CreateCompatibleBitmap.argtypes = [wintypes.HDC, ctypes.c_int, ctypes.c_int]
    gdi32.CreateCompatibleBitmap.restype = wintypes.HBITMAP

    gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HGDIOBJ]
    gdi32.SelectObject.restype = wintypes.HGDIOBJ
    gdi32.DeleteObject.argtypes = [wintypes.HGDIOBJ]
    gdi32.DeleteObject.restype = wintypes.BOOL

    gdi32.BitBlt.argtypes = [
        wintypes.HDC,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        wintypes.HDC,
        ctypes.c_int,
        ctypes.c_int,
        wintypes.DWORD,
    ]
    gdi32.BitBlt.restype = wintypes.BOOL

    gdi32.GetDIBits.argtypes = [
        wintypes.HDC,
        wintypes.HBITMAP,
        wintypes.UINT,
        wintypes.UINT,
        ctypes.c_void_p,
        ctypes.POINTER(_BITMAPINFO),
        wintypes.UINT,
    ]
    gdi32.GetDIBits.restype = ctypes.c_int

    # DWM signature
    dwmapi.DwmGetWindowAttribute.argtypes = [
        wintypes.HWND,
        wintypes.DWORD,
        ctypes.c_void_p,
        wintypes.DWORD,
    ]
    dwmapi.DwmGetWindowAttribute.restype = ctypes.c_long

    _WIN32_CACHE.update(
        {
            "kernel32": kernel32,
            "user32": user32,
            "gdi32": gdi32,
            "dwmapi": dwmapi,
            "enum_cb": enum_cb,
        }
    )
    return _WIN32_CACHE


def win32_mutex_exists(name: str) -> bool:
    w = _win32()
    h = w["kernel32"].OpenMutexW(SYNCHRONIZE, False, name)
    if h:
        w["kernel32"].CloseHandle(h)
        return True
    return False


def win32_event_exists(name: str) -> bool:
    w = _win32()
    h = w["kernel32"].OpenEventW(SYNCHRONIZE, False, name)
    if h:
        w["kernel32"].CloseHandle(h)
        return True
    return False


def win32_signal_event(name: str) -> None:
    w = _win32()
    k32 = w["kernel32"]
    ctypes.set_last_error(0)
    h = k32.OpenEventW(EVENT_MODIFY_STATE, False, name)
    if not h:
        err = ctypes.get_last_error()
        raise VerifyError(f"OpenEventW failed for {name!r} (Win32 error {err})")
    try:
        ctypes.set_last_error(0)
        if not k32.SetEvent(h):
            err = ctypes.get_last_error()
            raise VerifyError(f"SetEvent failed for {name!r} (Win32 error {err})")
    finally:
        k32.CloseHandle(h)


def win32_query_process(pid: int) -> dict[str, Any]:
    w = _win32()
    k32 = w["kernel32"]
    h = k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, False, pid)
    if not h:
        return {"pid": pid, "alive": False, "exe": None, "exit_code": None}
    try:
        code = wintypes.DWORD(0)
        alive = False
        if k32.GetExitCodeProcess(h, ctypes.byref(code)):
            alive = code.value == STILL_ACTIVE
        buf = ctypes.create_unicode_buffer(1024)
        size = wintypes.DWORD(len(buf))
        exe = None
        if k32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size)):
            exe = buf.value
        return {
            "pid": pid,
            "alive": alive,
            "exe": exe,
            "exit_code": None if alive else int(code.value),
        }
    finally:
        k32.CloseHandle(h)


def win32_terminate_verified_pid(pid: int, expected_exe: str) -> bool:
    info = win32_query_process(pid)
    if not info["alive"]:
        return False
    if not info["exe"] or not same_path(info["exe"], expected_exe):
        raise VerifyError(
            f"Refusing to terminate PID {pid}: exe mismatch "
            f"(actual={info['exe']!r}, expected={expected_exe!r})"
        )
    w = _win32()
    k32 = w["kernel32"]
    ctypes.set_last_error(0)
    h = k32.OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, False, pid)
    if not h:
        err = ctypes.get_last_error()
        raise VerifyError(f"OpenProcess(TERMINATE) failed for PID {pid} (Win32 error {err})")
    try:
        return bool(k32.TerminateProcess(h, 1))
    finally:
        k32.CloseHandle(h)


def win32_probe_clipboard() -> dict[str, Any]:
    w = _win32()
    u32 = w["user32"]
    ctypes.set_last_error(0)
    if u32.OpenClipboard(None):
        u32.CloseClipboard()
        return {"openable": True, "error_code": 0}
    return {"openable": False, "error_code": int(ctypes.get_last_error())}


def _window_text(hwnd: int) -> str:
    u32 = _win32()["user32"]
    buf = ctypes.create_unicode_buffer(512)
    u32.GetWindowTextW(hwnd, buf, len(buf))
    return buf.value


def _window_class(hwnd: int) -> str:
    u32 = _win32()["user32"]
    buf = ctypes.create_unicode_buffer(256)
    u32.GetClassNameW(hwnd, buf, len(buf))
    return buf.value


def _desktop_name(hdesk: int | None) -> str | None:
    if not hdesk:
        return None
    u32 = _win32()["user32"]
    buf = ctypes.create_unicode_buffer(256)
    needed = wintypes.DWORD(0)
    if not u32.GetUserObjectInformationW(
        hdesk, UOI_NAME, buf, ctypes.sizeof(buf), ctypes.byref(needed)
    ):
        return None
    return buf.value or None


def win32_desktop_info() -> dict[str, Any]:
    w = _win32()
    u32 = w["user32"]
    k32 = w["kernel32"]
    thread_h = u32.GetThreadDesktop(k32.GetCurrentThreadId())
    thread_name = _desktop_name(thread_h)
    input_h = u32.OpenInputDesktop(0, False, DESKTOP_READOBJECTS)
    input_name = None
    if input_h:
        try:
            input_name = _desktop_name(input_h)
        finally:
            u32.CloseDesktop(input_h)
    same = bool(thread_name and input_name and thread_name.lower() == input_name.lower())
    input_is_winlogon = bool(input_name and input_name.lower() == "winlogon")
    return {
        "thread_desktop": thread_name,
        "input_desktop": input_name,
        "same_as_input_desktop": same,
        "input_is_winlogon": input_is_winlogon,
    }


def win32_foreground_info() -> dict[str, Any]:
    w = _win32()
    u32 = w["user32"]
    desk = win32_desktop_info()
    hwnd = u32.GetForegroundWindow()
    if not hwnd:
        return {
            "hwnd": 0,
            "hwnd_hex": "0x0",
            "pid": 0,
            "process_name": None,
            "class_name": "",
            "title": "",
            "is_lock_screen": bool(desk["input_is_winlogon"]),
            **{k: desk[k] for k in ("thread_desktop", "input_desktop", "same_as_input_desktop")},
        }
    pid = wintypes.DWORD(0)
    u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    proc_info = win32_query_process(int(pid.value)) if pid.value else {"exe": None}
    proc_name = Path(proc_info["exe"]).name if proc_info.get("exe") else None
    cls = _window_class(hwnd)
    title = _window_text(hwnd)
    is_lock = bool(desk["input_is_winlogon"] or (proc_name and proc_name.lower() == "lockapp.exe"))
    return {
        "hwnd": int(hwnd),
        "hwnd_hex": hex(int(hwnd)),
        "pid": int(pid.value),
        "process_name": proc_name,
        "class_name": cls,
        "title": title,
        "is_lock_screen": is_lock,
        **{k: desk[k] for k in ("thread_desktop", "input_desktop", "same_as_input_desktop")},
    }


def win32_enum_windows_for_pid(target_pid: int) -> list[dict[str, Any]]:
    w = _win32()
    u32 = w["user32"]
    dwmapi = w["dwmapi"]
    results: list[dict[str, Any]] = []

    def callback(hwnd: int, _lparam: int) -> bool:
        pid = wintypes.DWORD(0)
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if int(pid.value) != target_pid:
            return True
        title = _window_text(hwnd)
        cls = _window_class(hwnd)
        visible = bool(u32.IsWindowVisible(hwnd))
        rect = _RECT()
        u32.GetWindowRect(hwnd, ctypes.byref(rect))
        ex_style = int(u32.GetWindowLongPtrW(hwnd, GWL_EXSTYLE))
        cloaked_val = ctypes.c_uint32(0)
        hr = dwmapi.DwmGetWindowAttribute(
            hwnd, DWMWA_CLOAKED, ctypes.byref(cloaked_val), ctypes.sizeof(cloaked_val)
        )
        cloaked = int(cloaked_val.value) if hr == 0 else 0
        dpi = int(u32.GetDpiForWindow(hwnd))
        left, top, right, bottom = int(rect.left), int(rect.top), int(rect.right), int(rect.bottom)
        width, height = right - left, bottom - top
        results.append(
            {
                "hwnd": int(hwnd),
                "hwnd_hex": hex(int(hwnd)),
                "title": title,
                "role": classify_window_role(title, cls, width),
                "class_name": cls,
                "visible": visible,
                "cloaked": cloaked,
                "topmost": bool(ex_style & WS_EX_TOPMOST),
                "dpi": dpi,
                "rect": {
                    "left": left,
                    "top": top,
                    "right": right,
                    "bottom": bottom,
                    "width": width,
                    "height": height,
                },
                "is_business": is_business_window(title, cls),
                "onscreen": is_window_onscreen(
                    visible=visible,
                    cloaked=cloaked,
                    left=left,
                    width=width,
                    height=height,
                ),
            }
        )
        return True

    cb_fn = w["enum_cb"](callback)
    u32.EnumWindows(cb_fn, 0)
    return results


def _bitmap_to_png(
    mem_dc: int,
    bmp: int,
    cw: int,
    ch: int,
    out_path: Path,
    *,
    zoom: int = 1,
    rect_meta: dict[str, int],
) -> dict[str, Any]:
    gdi32 = _win32()["gdi32"]
    bmi = _BITMAPINFO()
    bmi.bmiHeader.biSize = ctypes.sizeof(_BITMAPINFOHEADER)
    bmi.bmiHeader.biWidth = cw
    bmi.bmiHeader.biHeight = -ch  # top-down scanlines
    bmi.bmiHeader.biPlanes = 1
    bmi.bmiHeader.biBitCount = 32
    bmi.bmiHeader.biCompression = BI_RGB
    bmi.bmiHeader.biSizeImage = cw * ch * 4

    bgra = (ctypes.c_ubyte * (cw * ch * 4))()
    lines = gdi32.GetDIBits(mem_dc, bmp, 0, ch, bgra, ctypes.byref(bmi), DIB_RGB_COLORS)
    if lines != ch:
        raise VerifyError(f"GetDIBits copied {lines} scanlines, expected {ch}.")

    raw_bgra = bytes(bgra)
    rgb = bytearray(cw * ch * 3)
    rgb[0::3] = raw_bgra[2::4]
    rgb[1::3] = raw_bgra[1::4]
    rgb[2::3] = raw_bgra[0::4]

    stats = rgb_pixel_stats(rgb)
    scaled_rgb, out_w, out_h = upscale_rgb(rgb, cw, ch, zoom)
    png_bytes = encode_png_rgb(scaled_rgb, out_w, out_h)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(png_bytes)
    return {
        "capture_rect": rect_meta,
        "image_size": {"width": out_w, "height": out_h},
        "size_bytes": len(png_bytes),
        "pixel_stats": stats,
    }


def capture_rect_to_png(
    x: int, y: int, width: int, height: int, out_path: Path, *, zoom: int = 1
) -> dict[str, Any]:
    w = _win32()
    u32 = w["user32"]
    gdi32 = w["gdi32"]

    vx = int(u32.GetSystemMetrics(SM_XVIRTUALSCREEN))
    vy = int(u32.GetSystemMetrics(SM_YVIRTUALSCREEN))
    vw = int(u32.GetSystemMetrics(SM_CXVIRTUALSCREEN))
    vh = int(u32.GetSystemMetrics(SM_CYVIRTUALSCREEN))
    if vw <= 0 or vh <= 0:
        raise VerifyError("Virtual screen metrics returned non-positive size.")

    left = max(x, vx)
    top = max(y, vy)
    right = min(x + width, vx + vw)
    bottom = min(y + height, vy + vh)
    cw = right - left
    ch = bottom - top
    if cw <= 0 or ch <= 0:
        raise VerifyError(
            f"Capture rect ({x},{y},{width},{height}) does not intersect virtual screen ({vx},{vy},{vw},{vh})."
        )

    screen_dc = u32.GetDC(None)
    if not screen_dc:
        raise VerifyError("GetDC(NULL) failed.")
    mem_dc = None
    bmp = None
    old_obj = None
    try:
        mem_dc = gdi32.CreateCompatibleDC(screen_dc)
        if not mem_dc:
            raise VerifyError("CreateCompatibleDC failed.")
        bmp = gdi32.CreateCompatibleBitmap(screen_dc, cw, ch)
        if not bmp:
            raise VerifyError("CreateCompatibleBitmap failed.")
        old_obj = gdi32.SelectObject(mem_dc, bmp)
        if not old_obj:
            raise VerifyError("SelectObject failed.")
        if not gdi32.BitBlt(mem_dc, 0, 0, cw, ch, screen_dc, left, top, SRCCOPY_CAPTUREBLT):
            raise VerifyError(f"BitBlt failed (Win32 error {ctypes.get_last_error()}).")
        return _bitmap_to_png(
            mem_dc,
            bmp,
            cw,
            ch,
            out_path,
            zoom=zoom,
            rect_meta={"left": left, "top": top, "width": cw, "height": ch},
        )
    finally:
        if mem_dc and old_obj:
            gdi32.SelectObject(mem_dc, old_obj)
        if bmp:
            gdi32.DeleteObject(bmp)
        if mem_dc:
            gdi32.DeleteDC(mem_dc)
        u32.ReleaseDC(None, screen_dc)


def capture_window_to_png(
    hwnd: int, rect_meta: dict[str, int], out_path: Path, *, zoom: int = 1
) -> dict[str, Any]:
    w = _win32()
    u32 = w["user32"]
    gdi32 = w["gdi32"]
    cw = int(rect_meta["width"])
    ch = int(rect_meta["height"])
    if cw <= 0 or ch <= 0:
        raise VerifyError(f"Window rect has non-positive size: {rect_meta}")

    screen_dc = u32.GetDC(None)
    if not screen_dc:
        raise VerifyError("GetDC(NULL) failed.")
    mem_dc = None
    bmp = None
    old_obj = None
    try:
        mem_dc = gdi32.CreateCompatibleDC(screen_dc)
        if not mem_dc:
            raise VerifyError("CreateCompatibleDC failed.")
        bmp = gdi32.CreateCompatibleBitmap(screen_dc, cw, ch)
        if not bmp:
            raise VerifyError("CreateCompatibleBitmap failed.")
        old_obj = gdi32.SelectObject(mem_dc, bmp)
        if not old_obj:
            raise VerifyError("SelectObject failed.")
        if not u32.PrintWindow(hwnd, mem_dc, PW_RENDERFULLCONTENT):
            if not u32.PrintWindow(hwnd, mem_dc, 0):
                raise VerifyError(f"PrintWindow failed for HWND {hex(hwnd)} (Win32 error {ctypes.get_last_error()}).")
        return _bitmap_to_png(
            mem_dc,
            bmp,
            cw,
            ch,
            out_path,
            zoom=zoom,
            rect_meta=dict(rect_meta),
        )
    finally:
        if mem_dc and old_obj:
            gdi32.SelectObject(mem_dc, old_obj)
        if bmp:
            gdi32.DeleteObject(bmp)
        if mem_dc:
            gdi32.DeleteDC(mem_dc)
        u32.ReleaseDC(None, screen_dc)


def list_floatpaste_processes() -> list[dict[str, Any]]:
    cmd = [
        "pwsh",
        "-NoProfile",
        "-Command",
        (
            "$p = @(Get-CimInstance Win32_Process -Filter \"Name='floatpaste.exe'\" "
            "| Select-Object ProcessId,ExecutablePath,CommandLine); "
            "if ($p.Count -eq 0) { '[]' } else { $p | ConvertTo-Json -Compress }"
        ),
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=10)
    if proc.returncode != 0:
        raise VerifyError(f"Failed to query Win32_Process: {proc.stderr.strip()}")
    raw = proc.stdout.strip() or "[]"
    parsed = json.loads(raw)
    if isinstance(parsed, dict):
        parsed = [parsed]
    out: list[dict[str, Any]] = []
    for item in parsed:
        out.append(
            {
                "pid": int(item["ProcessId"]),
                "exe": item.get("ExecutablePath"),
                "cmdline": item.get("CommandLine"),
            }
        )
    return out


# ---------------------------------------------------------------------------
# Read-only SQLite + log + staleness inspection
# ---------------------------------------------------------------------------

def _open_ro_sqlite(db_path: Path) -> sqlite3.Connection:
    if not db_path.is_file():
        raise VerifyError(f"Database file does not exist: {db_path}")
    uri = f"{db_path.resolve().as_uri()}?mode=ro"
    return sqlite3.connect(uri, uri=True, timeout=5.0)


def json_safe_cell(value: Any) -> Any:
    if isinstance(value, (bytes, bytearray, memoryview)):
        raw = bytes(value)
        if len(raw) <= 4096:
            return {"type": "blob", "bytes": len(raw), "hex": raw.hex()}
        return {
            "type": "blob",
            "bytes": len(raw),
            "hex_preview": raw[:256].hex(),
            "truncated": True,
        }
    return value


def validate_readonly_sql(sql: str) -> str:
    stripped = sql.strip()
    if not stripped:
        raise VerifyError("SQL query must not be empty.")
    without_tail = stripped[:-1].strip() if stripped.endswith(";") else stripped
    if ";" in without_tail:
        raise VerifyError("Multiple SQL statements are rejected in --mode sql.")
    # Reject SQL comments so first keyword check cannot be spoofed
    if without_tail.startswith(("--", "/*")):
        raise VerifyError("Leading SQL comments are not allowed in --mode sql.")
    head = without_tail.split(None, 1)[0].lower()
    if head not in ("select", "pragma"):
        raise VerifyError(f"Only SELECT or read-only PRAGMA is allowed (got {head!r}).")
    if head == "pragma" and "=" in without_tail:
        raise VerifyError("Mutating PRAGMA assignments are not allowed in --mode sql.")
    return without_tail


def query_db(db_path: Path, mode: str, *, sql: str | None = None, limit: int = 20) -> dict[str, Any]:
    conn = _open_ro_sqlite(db_path)
    conn.row_factory = sqlite3.Row
    try:
        if mode == "summary":
            active = conn.execute(
                "SELECT COUNT(*) FROM clip_items WHERE deleted_at IS NULL"
            ).fetchone()[0]
            deleted = conn.execute(
                "SELECT COUNT(*) FROM clip_items WHERE deleted_at IS NOT NULL"
            ).fetchone()[0]
            favorited = conn.execute(
                "SELECT COUNT(*) FROM clip_items WHERE deleted_at IS NULL AND is_favorited = 1"
            ).fetchone()[0]
            tags = conn.execute("SELECT COUNT(*) FROM tags").fetchone()[0]
            excluded = conn.execute("SELECT COUNT(*) FROM excluded_apps").fetchone()[0]
            row = conn.execute(
                "SELECT value FROM settings WHERE key = 'user_settings'"
            ).fetchone()
            user_settings = json.loads(row[0]) if row else {}
            pos_row = conn.execute(
                "SELECT value FROM settings WHERE key = 'picker_last_position'"
            ).fetchone()
            picker_pos = json.loads(pos_row[0]) if pos_row else None
            return {
                "db_path": str(db_path),
                "active_items": int(active),
                "deleted_items": int(deleted),
                "favorited_active_items": int(favorited),
                "tags": int(tags),
                "excluded_apps": int(excluded),
                "pause_monitoring": bool(user_settings.get("pauseMonitoring", False)),
                "picker_position_mode": user_settings.get("pickerPositionMode"),
                "picker_last_position": picker_pos,
            }
        if mode == "items":
            rows = conn.execute(
                """
                SELECT ci.id, ci.type, ci.text_kind, ci.preview_text, ci.source_app,
                       ci.is_favorited, ci.created_at, ci.updated_at, ci.deleted_at,
                       COALESCE((
                           SELECT GROUP_CONCAT(tag_name, ',')
                           FROM clip_item_tags
                           WHERE item_id = ci.id
                       ), '') AS tags
                FROM clip_items ci
                ORDER BY ci.created_at DESC
                LIMIT ?
                """,
                (max(1, limit),),
            ).fetchall()
            return {"db_path": str(db_path), "items": [dict(r) for r in rows]}
        if mode == "settings":
            rows = conn.execute("SELECT key, value FROM settings ORDER BY key").fetchall()
            parsed: dict[str, Any] = {}
            for r in rows:
                try:
                    parsed[r["key"]] = json.loads(r["value"])
                except json.JSONDecodeError:
                    parsed[r["key"]] = r["value"]
            return {"db_path": str(db_path), "settings": parsed}
        if mode == "excluded":
            rows = conn.execute(
                "SELECT executable_name FROM excluded_apps ORDER BY executable_name"
            ).fetchall()
            return {
                "db_path": str(db_path),
                "excluded_apps": [r["executable_name"] for r in rows],
            }
        if mode == "sql":
            if not sql:
                raise VerifyError("--sql is required when --mode sql")
            safe_sql = validate_readonly_sql(sql)
            cur = conn.execute(safe_sql)
            cols = [desc[0] for desc in (cur.description or [])]
            rows = [
                {col: json_safe_cell(val) for col, val in zip(cols, row)}
                for row in cur.fetchmany(max(1, limit))
            ]
            return {"db_path": str(db_path), "columns": cols, "rows": rows}
        raise VerifyError(f"Unknown db mode: {mode!r}")
    finally:
        conn.close()


def inspect_logs(appdata_dir: Path, *, allow_hotkey_conflict: bool = False) -> dict[str, Any]:
    logs_dir = appdata_dir / "floatpaste" / "logs"
    files = sorted(logs_dir.glob("floatpaste-native.log*")) if logs_dir.is_dir() else []
    if not files:
        return {
            "log_path": None,
            "startup_diagnostic_found": False,
            "error_lines": [],
            "hotkey_conflict_lines": [],
        }
    latest = files[-1]
    text = latest.read_text(encoding="utf-8", errors="replace")
    lines = text.splitlines()
    startup_ok = any("应用启动诊断" in line for line in lines)
    all_errors = [line for line in lines if (" ERROR " in line or "panic" in line.lower())]
    hotkey_conflicts = [
        line
        for line in all_errors
        if "注册全局快捷键" in line and "0x80070581" in line
    ]
    if allow_hotkey_conflict:
        unexpected = [line for line in all_errors if line not in hotkey_conflicts][-10:]
    else:
        unexpected = all_errors[-10:]
    return {
        "log_path": str(latest),
        "startup_diagnostic_found": startup_ok,
        "error_lines": unexpected,
        "hotkey_conflict_lines": hotkey_conflicts[-10:],
    }


def check_binary_status(repo_root: Path, exe_path: Path) -> dict[str, Any]:
    if not exe_path.is_file():
        return {
            "path": str(exe_path),
            "exists": False,
            "size": 0,
            "mtime": None,
            "build_stale": True,
            "newest_source_path": None,
            "newest_source_mtime": None,
        }
    exe_stat = exe_path.stat()
    exe_mtime = exe_stat.st_mtime
    newest_path: Path | None = None
    newest_mtime = 0.0
    candidates: list[Path] = []
    for name in ("Cargo.toml", "Cargo.lock"):
        p = repo_root / name
        if p.is_file():
            candidates.append(p)
    crates_dir = repo_root / "crates"
    if crates_dir.is_dir():
        for ext in ("*.rs", "*.slint", "*.sql", "Cargo.toml", "*.png", "*.ico", "*.rc"):
            candidates.extend(
                p for p in crates_dir.rglob(ext) if "_preview_pngs" not in p.parts
            )
    for item in candidates:
        try:
            mt = item.stat().st_mtime
        except OSError:
            continue
        if mt > newest_mtime:
            newest_mtime = mt
            newest_path = item
    return {
        "path": str(exe_path),
        "exists": True,
        "size": int(exe_stat.st_size),
        "mtime": datetime.fromtimestamp(exe_mtime).isoformat(timespec="seconds"),
        "build_stale": bool(newest_mtime > exe_mtime + 1.0),
        "newest_source_path": str(newest_path) if newest_path else None,
        "newest_source_mtime": (
            datetime.fromtimestamp(newest_mtime).isoformat(timespec="seconds")
            if newest_mtime
            else None
        ),
    }


# ---------------------------------------------------------------------------
# Subcommand implementations
# ---------------------------------------------------------------------------

def _session_health(
    repo_root: Path, session: dict[str, Any], *, allow_hotkey_conflict: bool = False
) -> dict[str, Any]:
    iid = str(session["instance_id"])
    pid = int(session["pid"])
    exe = str(session["exe"])
    appdata_dir = Path(session["appdata_dir"])
    db_path = appdata_dir / "floatpaste" / "floatpaste.db"
    proc = win32_query_process(pid)
    exe_matches = bool(proc["exe"] and same_path(proc["exe"], exe))
    windows = win32_enum_windows_for_pid(pid) if proc["alive"] and exe_matches else []
    db_readable = False
    db_summary: dict[str, Any] | None = None
    if db_path.is_file():
        try:
            db_summary = query_db(db_path, "summary")
            db_readable = True
        except Exception:
            db_readable = False
    logs = inspect_logs(appdata_dir, allow_hotkey_conflict=allow_hotkey_conflict)
    mutex_open = win32_mutex_exists(mutex_name_for(iid))
    wake_open = win32_event_exists(event_name_for("wake", iid))
    exit_open = win32_event_exists(event_name_for("exit", iid))
    current_desktop = win32_desktop_info()["thread_desktop"]
    launch_desktop = session.get("launch_desktop")
    desktop_mismatch = bool(
        launch_desktop and current_desktop and str(launch_desktop).lower() != str(current_desktop).lower()
    )
    return {
        "run_id": session["run_id"],
        "instance_id": iid,
        "pid": pid,
        "alive": bool(proc["alive"]),
        "exe_matches": exe_matches,
        "launch_desktop": launch_desktop,
        "current_desktop": current_desktop,
        "desktop_mismatch": desktop_mismatch,
        "mutex_openable": mutex_open,
        "wake_event_openable": wake_open,
        "exit_event_openable": exit_open,
        "window_count": len(windows),
        "windows": windows,
        "db_readable": db_readable,
        "db_summary": db_summary,
        "logs": logs,
    }


def cmd_doctor(args: argparse.Namespace, repo_root: Path) -> tuple[int, dict[str, Any]]:
    exe_path = Path(args.exe) if args.exe else repo_root / "target" / "debug" / "floatpaste.exe"
    daily_state_file = artifacts_base(repo_root) / "daily-state.json"
    daily = {
        "mutex_active": win32_mutex_exists(DAILY_MUTEX_NAME),
        "processes": list_floatpaste_processes(),
        "pause_state_file_exists": daily_state_file.is_file(),
    }
    clipboard = win32_probe_clipboard()
    desktop = win32_foreground_info()
    binary = check_binary_status(repo_root, exe_path)

    session_report = None
    session_ok = True
    warnings: list[str] = []
    if binary["build_stale"]:
        warnings.append("Target binary is older than workspace sources; rebuild before testing new changes.")

    should_check_session = bool(args.session)
    if not should_check_session:
        latest = artifacts_base(repo_root) / "latest.txt"
        if latest.is_file() and latest.read_text(encoding="utf-8-sig").strip():
            should_check_session = True

    if should_check_session:
        session = load_session(repo_root, args.session)
        session_report = _session_health(
            repo_root,
            session,
            allow_hotkey_conflict=bool(daily["mutex_active"]),
        )
        session_ok = (
            session_report["alive"]
            and session_report["exe_matches"]
            and session_report["mutex_openable"]
            and session_report["wake_event_openable"]
            and session_report["exit_event_openable"]
            and session_report["db_readable"]
            and not session_report["logs"]["error_lines"]
        )
        if session_report["logs"]["hotkey_conflict_lines"]:
            warnings.append(
                "Global hotkey registration returned ERROR_HOTKEY_ALREADY_REGISTERED (0x80070581) "
                "because the daily FloatPaste instance holds session global hotkeys."
            )
        if session_report["desktop_mismatch"]:
            warnings.append(
                f"Session was launched on desktop {session_report['launch_desktop']!r} while doctor runs on "
                f"{session_report['current_desktop']!r}; EnumWindows/PrintWindow only see windows on "
                "launch_desktop, so on ephemeral exebox-* desktops run launch/windows/capture in one shell command."
            )

    if daily["mutex_active"]:
        warnings.append(
            "Daily FloatPaste instance is running; clipboard-writing checks must stay paused or use `daily pause` first."
        )
    if desktop["is_lock_screen"]:
        warnings.append("Interactive desktop is locked (Winlogon / LockApp active).")
    elif not desktop["same_as_input_desktop"]:
        warnings.append(
            f"Running on isolated Windows desktop ({desktop['thread_desktop']}); "
            f"input desktop is {desktop['input_desktop']}. Isolated process, named events, geometry, "
            "SQLite, and PrintWindow capture work without touching the user's visible desktop; "
            "run on Default desktop for true DWM backdrop compositing."
        )
    elif desktop["hwnd"] == 0:
        warnings.append("No active foreground window on current desktop.")

    healthy = bool(binary["exists"]) and session_ok
    payload = {
        "ok": healthy,
        "warnings": warnings,
        "daily_instance": daily,
        "clipboard": clipboard,
        "desktop": desktop,
        "binary": binary,
        "session": session_report,
    }
    return (0 if healthy else 1), payload


def _verified_session_pid(session: dict[str, Any]) -> tuple[int, str, str]:
    pid = int(session["pid"])
    exe = str(session["exe"])
    iid = str(session["instance_id"])
    proc = win32_query_process(pid)
    if not proc["alive"]:
        raise VerifyError(f"Isolated instance PID {pid} is not running.")
    if not proc["exe"] or not same_path(proc["exe"], exe):
        raise VerifyError(
            f"PID {pid} executable mismatch (actual={proc['exe']!r}, expected={exe!r}); refusing signal."
        )
    return pid, exe, iid


def cmd_launch(args: argparse.Namespace, repo_root: Path) -> tuple[int, dict[str, Any]]:
    exe_path = Path(args.exe) if args.exe else repo_root / "target" / "debug" / "floatpaste.exe"
    if not exe_path.is_file():
        raise VerifyError(
            f"Executable not found: {exe_path}. Build first with `cargo build -p floatpaste-native`."
        )

    raw_id = args.run_id or f"{datetime.now().strftime('%Y%m%d-%H%M%S')}-{uuid.uuid4().hex[:6]}"
    run_id = sanitize_instance_id(raw_id)
    instance_id = sanitize_instance_id(f"vf-{run_id}")

    picker_pos: tuple[int, int] | None = None
    if args.picker_pos:
        picker_pos = parse_xy_point(args.picker_pos)

    base = artifacts_base(repo_root)
    base.mkdir(parents=True, exist_ok=True)
    run_root, runtime_dir, evidence_dir = run_dirs(repo_root, run_id)
    if run_root.exists():
        raise VerifyError(f"Run directory already exists (exclusive creation required): {run_root}")

    appdata_dir = runtime_dir / "appdata"
    appdata_dir.mkdir(parents=True, exist_ok=False)
    if picker_pos is not None:
        seed_last_position_db(appdata_dir, picker_pos[0], picker_pos[1])
    evidence_dir.mkdir(parents=True, exist_ok=False)
    stdout_log = runtime_dir / "stdout.log"
    stderr_log = runtime_dir / "stderr.log"

    env = os.environ.copy()
    for var in ISOLATED_ENV_CLEAR:
        env.pop(var, None)
    env["APPDATA"] = str(appdata_dir)
    env["FLOATPASTE_INSTANCE_ID"] = instance_id
    env["FLOATPASTE_SKIP_AUTOSTART_SYNC"] = "1"
    if args.diagnostic_log:
        env["FLOATPASTE_DIAGNOSTIC_LOG"] = "1"
    if args.retention_delay_ms is not None:
        env["FLOATPASTE_RETENTION_FIRST_DELAY_MS"] = str(args.retention_delay_ms)

    cmd = [str(exe_path)]
    if args.silent:
        cmd.append("--silent")

    out_fh = stdout_log.open("w", encoding="utf-8")
    err_fh = stderr_log.open("w", encoding="utf-8")
    try:
        proc = subprocess.Popen(
            cmd,
            env=env,
            cwd=str(repo_root),
            stdin=subprocess.DEVNULL,
            stdout=out_fh,
            stderr=err_fh,
            creationflags=CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW,
        )
    finally:
        out_fh.close()
        err_fh.close()

    pid = proc.pid
    db_path = appdata_dir / "floatpaste" / "floatpaste.db"
    wake_ev = event_name_for("wake", instance_id)
    exit_ev = event_name_for("exit", instance_id)
    deadline = time.monotonic() + float(args.timeout)
    ready = False
    last_reason = "timed out waiting for readiness"

    while time.monotonic() < deadline:
        if proc.poll() is not None:
            last_reason = f"process exited early with code {proc.returncode}"
            break
        if not (win32_event_exists(wake_ev) and win32_event_exists(exit_ev)):
            last_reason = "named events not yet created"
            time.sleep(0.1)
            continue
        if not db_path.is_file():
            last_reason = "database file not yet created"
            time.sleep(0.1)
            continue
        try:
            _ = query_db(db_path, "summary")
        except Exception as exc:
            last_reason = f"database not yet readable: {exc}"
            time.sleep(0.1)
            continue
        wins = win32_enum_windows_for_pid(pid)
        roles = {w["role"] for w in wins}
        if not ("picker" in roles and "search" in roles):
            last_reason = f"expected picker and search windows, saw roles={sorted(roles)}"
            time.sleep(0.1)
            continue
        ready = True
        break

    rogue_windows: list[dict[str, Any]] = []
    if ready:
        # Give event-loop first-frame assembly a brief settle window, then check for cold-start flash.
        time.sleep(0.2)
        wins = win32_enum_windows_for_pid(pid)
        rogue_windows = [w for w in wins if w["is_business"] and w["onscreen"]]
        if rogue_windows:
            ready = False
            details = [
                f"{w['hwnd_hex']}(title={w['title']!r},class={w['class_name']!r},rect={w['rect']})"
                for w in rogue_windows
            ]
            last_reason = f"cold-start rogue onscreen window(s): {details}"

    if not ready:
        try:
            if win32_event_exists(exit_ev):
                win32_signal_event(exit_ev)
        except Exception:
            pass
        try:
            proc.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            try:
                win32_terminate_verified_pid(pid, str(exe_path))
                proc.wait(timeout=3.0)
            except Exception:
                pass
        logs = inspect_logs(appdata_dir)
        raise VerifyError(f"Launch failed: {last_reason}; logs={logs}")

    session = {
        "run_id": run_id,
        "instance_id": instance_id,
        "pid": pid,
        "exe": str(exe_path.resolve()),
        "launch_desktop": win32_desktop_info()["thread_desktop"],
        "appdata_dir": str(appdata_dir.resolve()),
        "runtime_dir": str(runtime_dir.resolve()),
        "evidence_dir": str(evidence_dir.resolve()),
        "started_at": datetime.now().isoformat(timespec="seconds"),
    }
    (runtime_dir / "session.json").write_text(
        json.dumps(session, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    (base / "latest.txt").write_text(run_id, encoding="utf-8")

    db_summary = query_db(db_path, "summary")
    return 0, {
        "ok": True,
        **session,
        "cold_start_rogue_windows": rogue_windows,
        "window_roles": sorted({w["role"] for w in win32_enum_windows_for_pid(pid)}),
        "db_summary": db_summary,
    }


def cmd_signal(args: argparse.Namespace, repo_root: Path) -> tuple[int, dict[str, Any]]:
    session = load_session(repo_root, args.session)
    pid, exe, iid = _verified_session_pid(session)
    action = args.action
    if action == "wake" and args.via_second_launch:
        env = os.environ.copy()
        for var in ISOLATED_ENV_CLEAR:
            env.pop(var, None)
        env["APPDATA"] = str(session["appdata_dir"])
        env["FLOATPASTE_INSTANCE_ID"] = iid
        env["FLOATPASTE_SKIP_AUTOSTART_SYNC"] = "1"
        proc = subprocess.run(
            [exe],
            env=env,
            cwd=str(repo_root),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=float(args.timeout),
            creationflags=CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW,
        )
        if proc.returncode != 0:
            raise VerifyError(
                f"Second launch wake exited with code {proc.returncode}: {proc.stderr.strip()}"
            )
        return 0, {
            "ok": True,
            "action": "wake",
            "mode": "second-launch",
            "target_pid": pid,
            "second_launch_exit_code": proc.returncode,
        }

    ev_name = event_name_for(action, iid)
    win32_signal_event(ev_name)
    return 0, {
        "ok": True,
        "action": action,
        "mode": "named-event",
        "event_name": ev_name,
        "target_pid": pid,
    }


def cmd_windows(args: argparse.Namespace, repo_root: Path) -> tuple[int, dict[str, Any]]:
    session = load_session(repo_root, args.session)
    pid, _, _ = _verified_session_pid(session)
    current_desk = win32_desktop_info()["thread_desktop"]
    launch_desk = session.get("launch_desktop")
    mismatch = bool(launch_desk and current_desk and str(launch_desk).lower() != str(current_desk).lower())
    if not args.wait_role:
        wins = win32_enum_windows_for_pid(pid)
        out: dict[str, Any] = {
            "ok": True,
            "pid": pid,
            "launch_desktop": launch_desk,
            "current_desktop": current_desk,
            "desktop_mismatch": mismatch,
            "windows": wins,
        }
        if mismatch:
            out["hint"] = (
                "Current desktop differs from launch_desktop; on ephemeral exebox-* desktops, "
                "run launch/windows/capture inside the same shell command."
            )
        return 0, out

    expect = args.expect_state or "onscreen"
    want_onscreen = expect == "onscreen"
    deadline = time.monotonic() + float(args.timeout)
    last_matches: list[dict[str, Any]] = []
    while True:
        wins = win32_enum_windows_for_pid(pid)
        matches = [w for w in wins if w["role"] == args.wait_role]
        last_matches = matches
        if want_onscreen:
            hit = [w for w in matches if w["onscreen"]]
        else:
            hit = [w for w in matches if is_window_parked(w)]
        if hit:
            return 0, {
                "ok": True,
                "pid": pid,
                "role": args.wait_role,
                "expect_state": expect,
                "desktop_mismatch": mismatch,
                "matched": hit[0],
                "windows": wins,
            }
        if not matches and mismatch:
            return 1, {
                "ok": False,
                "pid": pid,
                "role": args.wait_role,
                "expect_state": expect,
                "launch_desktop": launch_desk,
                "current_desktop": current_desk,
                "desktop_mismatch": True,
                "error": (
                    f"Desktop mismatch: session runs on {launch_desk!r}, current command runs on "
                    f"{current_desk!r} (EnumWindows sees 0 matching windows). Run launch/windows/capture "
                    "within the same shell invocation."
                ),
                "role_matches": [],
            }
        if time.monotonic() >= deadline:
            return 1, {
                "ok": False,
                "pid": pid,
                "role": args.wait_role,
                "expect_state": expect,
                "desktop_mismatch": mismatch,
                "error": f"Timed out waiting for role={args.wait_role!r} state={expect!r}",
                "role_matches": last_matches,
            }
        time.sleep(0.1)


def cmd_foreground(_args: argparse.Namespace, _repo_root: Path) -> tuple[int, dict[str, Any]]:
    return 0, win32_foreground_info()


def cmd_capture(args: argparse.Namespace, repo_root: Path) -> tuple[int, dict[str, Any]]:
    session = load_session(repo_root, args.session)
    evidence_dir = Path(session["evidence_dir"])
    out_path = ensure_under(evidence_dir / args.out, evidence_dir)

    role_used = args.role or "rect"
    hwnd: int | None = None
    rect_meta: dict[str, int] | None = None
    if args.role:
        pid, _, _ = _verified_session_pid(session)
        wins = win32_enum_windows_for_pid(pid)
        candidates = [w for w in wins if w["role"] == args.role and w["onscreen"]]
        if not candidates:
            all_role = [w for w in wins if w["role"] == args.role]
            raise VerifyError(
                f"No onscreen window for role={args.role!r}. Current role windows: {all_role}"
            )
        win = candidates[0]
        hwnd = int(win["hwnd"])
        r = win["rect"]
        x, y, width, height = int(r["left"]), int(r["top"]), int(r["width"]), int(r["height"])
        rect_meta = {"left": x, "top": y, "width": width, "height": height}
        dpi = int(win["dpi"])
    else:
        parts = [p.strip() for p in args.rect.split(",")]
        if len(parts) != 4:
            raise VerifyError(f"--rect must be x,y,w,h; got {args.rect!r}")
        x, y, width, height = (int(p) for p in parts)
        dpi = 0

    fg_before = win32_foreground_info()
    method = choose_capture_method(
        args.method,
        has_hwnd=hwnd is not None,
        same_as_input_desktop=bool(fg_before["same_as_input_desktop"]),
    )
    if method == "print_window":
        assert hwnd is not None and rect_meta is not None
        cap_info = capture_window_to_png(hwnd, rect_meta, out_path, zoom=args.zoom)
    else:
        cap_info = capture_rect_to_png(x, y, width, height, out_path, zoom=args.zoom)
    entry = {
        "ts": datetime.now().isoformat(timespec="seconds"),
        "run_id": session["run_id"],
        "feature": args.feature or "",
        "entry_point": args.entry_point or "",
        "action": args.action or "",
        "role": role_used,
        "capture_method": method,
        "same_as_input_desktop": bool(fg_before["same_as_input_desktop"]),
        "dpi": dpi,
        "zoom": int(args.zoom),
        "path": out_path.relative_to(evidence_dir.resolve()).as_posix(),
        **cap_info,
        "foreground": {
            "pid": fg_before["pid"],
            "process_name": fg_before["process_name"],
            "title": fg_before["title"],
        },
    }
    manifest = evidence_dir / "manifest.jsonl"
    with manifest.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(entry, ensure_ascii=False) + "\n")
    return 0, {"ok": True, "file": str(out_path), **entry}


def cmd_db(args: argparse.Namespace, repo_root: Path) -> tuple[int, dict[str, Any]]:
    session = load_session(repo_root, args.session)
    db_path = Path(session["appdata_dir"]) / "floatpaste" / "floatpaste.db"
    result = query_db(db_path, args.mode, sql=args.sql, limit=args.limit)
    return 0, {"ok": True, "run_id": session["run_id"], **result}


def collect_known_isolated_pids(repo_root: Path) -> set[int]:
    pids: set[int] = set()
    base = artifacts_base(repo_root)
    if not base.is_dir():
        return pids
    for sfile in base.glob("*/runtime/session.json"):
        try:
            data = json.loads(sfile.read_text(encoding="utf-8-sig"))
        except (OSError, json.JSONDecodeError):
            continue
        if isinstance(data, dict) and isinstance(data.get("pid"), int):
            pids.add(int(data["pid"]))
    return pids


def select_daily_instances(
    procs: list[dict[str, Any]],
    *,
    mutex_active: bool,
    isolated_pids: set[int],
    target_pid: int | None = None,
) -> tuple[list[dict[str, Any]], str | None]:
    if not mutex_active:
        return [], None
    candidates = [p for p in procs if int(p["pid"]) not in isolated_pids]
    if target_pid is not None:
        matched = [p for p in candidates if int(p["pid"]) == target_pid]
        if not matched:
            return (
                [],
                f"Specified --pid {target_pid} was not found among non-isolated floatpaste.exe processes "
                f"(candidates={[p['pid'] for p in candidates]}, isolated={sorted(isolated_pids)}).",
            )
        return matched, None
    if len(candidates) == 1:
        return candidates, None
    if len(candidates) == 0:
        return (
            [],
            "Daily mutex is active, but no non-isolated floatpaste.exe process matched; "
            "please exit daily FloatPaste from tray manually.",
        )
    return (
        [],
        f"Multiple candidate floatpaste.exe processes ({[p['pid'] for p in candidates]}) found while daily mutex "
        "is active; refusing to guess. Stop extra isolated instances or pass --pid <daily_pid>.",
    )


def cmd_daily(args: argparse.Namespace, repo_root: Path) -> tuple[int, dict[str, Any]]:
    base = artifacts_base(repo_root)
    base.mkdir(parents=True, exist_ok=True)
    state_file = base / "daily-state.json"
    target_pid = getattr(args, "pid", None)

    if args.action == "status":
        procs = list_floatpaste_processes()
        isolated_pids = collect_known_isolated_pids(repo_root)
        daily_candidates = [p for p in procs if int(p["pid"]) not in isolated_pids]
        return 0, {
            "ok": True,
            "mutex_active": win32_mutex_exists(DAILY_MUTEX_NAME),
            "processes": procs,
            "isolated_pids": sorted(isolated_pids),
            "daily_candidates": daily_candidates,
            "state_file": str(state_file),
            "paused_recorded": state_file.is_file(),
        }

    if args.action == "pause":
        if state_file.is_file():
            raise VerifyError(
                f"Pause state already recorded at {state_file}. Run `daily restore` or remove stale state first."
            )
        procs = list_floatpaste_processes()
        mutex_active = win32_mutex_exists(DAILY_MUTEX_NAME)
        isolated_pids = collect_known_isolated_pids(repo_root)
        selected, sel_err = select_daily_instances(
            procs,
            mutex_active=mutex_active,
            isolated_pids=isolated_pids,
            target_pid=target_pid,
        )
        if sel_err:
            return 2, {
                "ok": False,
                "error": sel_err,
                "processes": procs,
                "isolated_pids": sorted(isolated_pids),
            }
        if not selected:
            payload = {
                "was_running": False,
                "paused_at": datetime.now().isoformat(timespec="seconds"),
                "instances": [],
            }
            state_file.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
            return 0, {"ok": True, "was_running": False, "state_file": str(state_file)}

        state_payload = {
            "was_running": True,
            "paused_at": datetime.now().isoformat(timespec="seconds"),
            "instances": selected,
        }
        state_file.write_text(
            json.dumps(state_payload, ensure_ascii=False, indent=2), encoding="utf-8"
        )

        seen_exes: set[str] = set()
        signaled: list[str] = []
        for proc in selected:
            exe = proc.get("exe")
            if not exe:
                continue
            key = os.path.normcase(exe)
            if key in seen_exes:
                continue
            seen_exes.add(key)
            ev = install_shutdown_event_name(exe)
            if win32_event_exists(ev):
                win32_signal_event(ev)
                signaled.append(ev)

        deadline = time.monotonic() + float(args.timeout)
        while time.monotonic() < deadline:
            still_up = [p for p in selected if win32_query_process(int(p["pid"]))["alive"]]
            if not still_up and not win32_mutex_exists(DAILY_MUTEX_NAME):
                return 0, {
                    "ok": True,
                    "was_running": True,
                    "signaled_events": signaled,
                    "state_file": str(state_file),
                }
            time.sleep(0.2)

        return 2, {
            "ok": False,
            "error": (
                "Daily FloatPaste did not exit gracefully before timeout; refusing to force-kill. "
                "Please exit from tray icon and re-check with `daily status`."
            ),
            "signaled_events": signaled,
            "state_file": str(state_file),
        }

    if args.action == "restore":
        if not state_file.is_file():
            raise VerifyError(f"No recorded daily pause state at {state_file}")
        saved = json.loads(state_file.read_text(encoding="utf-8-sig"))
        if not saved.get("was_running"):
            state_file.unlink()
            return 0, {"ok": True, "restored": False, "reason": "daily instance was not running"}

        clip = win32_probe_clipboard()
        if not clip["openable"]:
            return 2, {
                "ok": False,
                "error": f"System clipboard is not openable before restoring daily instance: {clip}",
            }
        if win32_mutex_exists(DAILY_MUTEX_NAME):
            state_file.unlink()
            return 0, {"ok": True, "restored": False, "reason": "daily mutex already active"}

        instances = saved.get("instances") or []
        if not instances or not instances[0].get("exe"):
            raise VerifyError(f"Recorded daily state has no executable path: {saved}")
        exe = str(instances[0]["exe"])
        extra_args = parse_cmdline_args(instances[0].get("cmdline"))
        if not Path(exe).is_file():
            raise VerifyError(f"Recorded daily executable no longer exists: {exe}")

        child = subprocess.Popen(
            [exe, *extra_args],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            creationflags=CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS,
        )
        deadline = time.monotonic() + 10.0
        while time.monotonic() < deadline:
            if win32_mutex_exists(DAILY_MUTEX_NAME):
                state_file.unlink()
                return 0, {
                    "ok": True,
                    "restored": True,
                    "pid": child.pid,
                    "exe": exe,
                    "args": extra_args,
                }
            time.sleep(0.2)
        raise VerifyError("Started daily FloatPaste but single-instance mutex did not activate in time.")

    raise VerifyError(f"Unknown daily action: {args.action!r}")


def cleanup_run_artifacts(
    repo_root: Path,
    session: dict[str, Any],
    *,
    keep_runtime: bool = False,
    exit_timeout: float = 10.0,
    stop_process_cb: Any | None = None,
) -> dict[str, Any]:
    """Stop tracked instance if alive, remove runtime/, and verify evidence/ survival."""
    base = artifacts_base(repo_root)
    run_id = sanitize_instance_id(str(session["run_id"]))
    iid = sanitize_instance_id(str(session["instance_id"]))
    pid = int(session["pid"])
    exe = str(session["exe"])
    runtime_dir = ensure_under(Path(session["runtime_dir"]), base)
    evidence_dir = ensure_under(Path(session["evidence_dir"]), base)
    _, expected_runtime, expected_evidence = run_dirs(repo_root, run_id)
    if (
        not same_path(runtime_dir, expected_runtime)
        or not same_path(evidence_dir, expected_evidence)
        or same_path(runtime_dir, evidence_dir)
    ):
        raise VerifyError(
            f"Refusing cleanup with inconsistent session paths for run {run_id!r}: "
            f"runtime_dir={runtime_dir} (expected {expected_runtime}), "
            f"evidence_dir={evidence_dir} (expected {expected_evidence})"
        )

    forced_kill = False
    already_exited = False

    if stop_process_cb is not None:
        already_exited, forced_kill = stop_process_cb(pid, exe, iid, exit_timeout)
    else:
        proc = win32_query_process(pid)
        if not proc["alive"]:
            already_exited = True
        else:
            if not proc["exe"] or not same_path(proc["exe"], exe):
                raise VerifyError(
                    f"PID {pid} executable mismatch during cleanup (actual={proc['exe']!r}, expected={exe!r})."
                )
            exit_ev = event_name_for("exit", iid)
            if win32_event_exists(exit_ev):
                win32_signal_event(exit_ev)
            deadline = time.monotonic() + exit_timeout
            while time.monotonic() < deadline:
                if not win32_query_process(pid)["alive"]:
                    break
                time.sleep(0.15)
            if win32_query_process(pid)["alive"]:
                win32_terminate_verified_pid(pid, exe)
                forced_kill = True
                wait_end = time.monotonic() + 3.0
                while time.monotonic() < wait_end and win32_query_process(pid)["alive"]:
                    time.sleep(0.1)

    runtime_removed = False
    if not keep_runtime and runtime_dir.exists():
        # Briefly retry on Windows transient file handle release right after process exit.
        for attempt in range(5):
            try:
                shutil.rmtree(runtime_dir)
                runtime_removed = True
                break
            except OSError:
                if attempt == 4:
                    raise
                time.sleep(0.2)

    latest_file = base / "latest.txt"
    if latest_file.is_file() and latest_file.read_text(encoding="utf-8-sig").strip() == run_id:
        latest_file.unlink()

    if not evidence_dir.is_dir():
        raise VerifyError(f"Evidence directory missing after cleanup: {evidence_dir}")

    evidence_files: list[dict[str, Any]] = []
    for item in sorted(evidence_dir.rglob("*")):
        if item.is_file():
            evidence_files.append(
                {
                    "path": item.relative_to(evidence_dir).as_posix(),
                    "bytes": item.stat().st_size,
                }
            )

    return {
        "ok": True,
        "run_id": run_id,
        "pid": pid,
        "already_exited": already_exited,
        "forced_kill": forced_kill,
        "runtime_removed": runtime_removed,
        "evidence_dir": str(evidence_dir),
        "evidence_preserved": evidence_dir.is_dir(),
        "evidence_files": evidence_files,
    }


def cmd_cleanup(args: argparse.Namespace, repo_root: Path) -> tuple[int, dict[str, Any]]:
    session = load_session(repo_root, args.session)
    res = cleanup_run_artifacts(
        repo_root,
        session,
        keep_runtime=bool(args.keep_runtime),
        exit_timeout=float(args.timeout),
    )
    return 0, res


# ---------------------------------------------------------------------------
# CLI entrypoint
# ---------------------------------------------------------------------------

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="FloatPaste cold-start desktop verification harness")
    parser.add_argument("--repo-root", default=None, help="Override repository root directory")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_doc = sub.add_parser("doctor", help="Read-only health check")
    p_doc.add_argument("--session", default=None, help="Run ID (defaults to latest if present)")
    p_doc.add_argument("--exe", default=None, help="Target floatpaste.exe path")

    p_launch = sub.add_parser("launch", help="Launch an isolated FloatPaste instance")
    p_launch.add_argument("--exe", default=None, help="Target floatpaste.exe path")
    p_launch.add_argument("--run-id", default=None, help="Explicit run ID (default: timestamp-rand)")
    p_launch.add_argument("--diagnostic-log", action="store_true", help="Enable FLOATPASTE_DIAGNOSTIC_LOG=1")
    p_launch.add_argument("--retention-delay-ms", type=int, default=None, help="Override retention initial delay ms")
    p_launch.add_argument("--picker-pos", default=None, help="Pre-seed LastPosition mode at x,y (e.g. 80,80)")
    p_launch.add_argument("--silent", action="store_true", help="Pass --silent to floatpaste.exe")
    p_launch.add_argument("--timeout", type=float, default=15.0, help="Readiness wait timeout seconds")

    p_sig = sub.add_parser("signal", help="Trigger wake or test event on isolated instance")
    p_sig.add_argument(
        "action",
        choices=["wake", *sorted(TEST_EVENT_BASES.keys())],
        help="Signal or event action",
    )
    p_sig.add_argument("--session", default=None, help="Run ID (defaults to latest)")
    p_sig.add_argument("--via-second-launch", action="store_true", help="For wake: spawn second instance with isolated env")
    p_sig.add_argument("--timeout", type=float, default=10.0, help="Second-launch exit timeout seconds")

    p_win = sub.add_parser("windows", help="List or wait for isolated instance windows")
    p_win.add_argument("--session", default=None, help="Run ID (defaults to latest)")
    p_win.add_argument("--wait-role", default=None, help="Wait for window role (picker/search/editor/settings/tooltip/tray)")
    p_win.add_argument("--expect-state", choices=["onscreen", "parked"], default="onscreen")
    p_win.add_argument("--timeout", type=float, default=10.0)

    sub.add_parser("foreground", help="Inspect current OS foreground window")

    p_cap = sub.add_parser("capture", help="Capture PNG screenshot into evidence/")
    p_cap.add_argument("--session", default=None, help="Run ID (defaults to latest)")
    grp = p_cap.add_mutually_exclusive_group(required=True)
    grp.add_argument("--role", help="Capture onscreen window of this role")
    grp.add_argument("--rect", help="Capture explicit screen rect x,y,w,h")
    p_cap.add_argument("--out", required=True, help="Relative output path under evidence/")
    p_cap.add_argument("--feature", default="", help="Feature ID recorded in manifest.jsonl")
    p_cap.add_argument("--entry-point", default="", help="Entry point recorded in manifest.jsonl")
    p_cap.add_argument("--action", default="", help="Action description recorded in manifest.jsonl")
    p_cap.add_argument("--zoom", type=int, default=1, help="Integer upscale factor (default 1)")
    p_cap.add_argument(
        "--method",
        choices=["auto", "screen", "window"],
        default="auto",
        help="Capture method: auto (PrintWindow on isolated desktop, screen BitBlt on Default desktop), screen, or window",
    )

    p_db = sub.add_parser("db", help="Read-only SQLite query against isolated floatpaste.db")
    p_db.add_argument("--session", default=None, help="Run ID (defaults to latest)")
    p_db.add_argument("--mode", choices=["summary", "items", "settings", "excluded", "sql"], default="summary")
    p_db.add_argument("--sql", default=None, help="Read-only SELECT/PRAGMA query for --mode sql")
    p_db.add_argument("--limit", type=int, default=20, help="Row limit for items/sql")

    p_daily = sub.add_parser("daily", help="Inspect, gracefully pause, or restore daily FloatPaste instance")
    p_daily.add_argument("action", choices=["status", "pause", "restore"])
    p_daily.add_argument("--pid", type=int, default=None, help="Explicit daily PID when multiple instances exist")
    p_daily.add_argument("--timeout", type=float, default=15.0)

    p_cln = sub.add_parser("cleanup", help="Gracefully stop isolated instance and remove runtime/ while preserving evidence/")
    p_cln.add_argument("--session", default=None, help="Run ID (defaults to latest)")
    p_cln.add_argument("--keep-runtime", action="store_true", help="Keep runtime/ directory for failure forensics")
    p_cln.add_argument("--timeout", type=float, default=10.0)

    return parser


def main(argv: list[str] | None = None) -> int:
    configure_stdio()
    args = build_parser().parse_args(argv)
    repo_root = Path(args.repo_root).resolve() if args.repo_root else default_repo_root()

    dispatch = {
        "doctor": cmd_doctor,
        "launch": cmd_launch,
        "signal": cmd_signal,
        "windows": cmd_windows,
        "foreground": cmd_foreground,
        "capture": cmd_capture,
        "db": cmd_db,
        "daily": cmd_daily,
        "cleanup": cmd_cleanup,
    }
    try:
        rc, payload = dispatch[args.cmd](args, repo_root)
    except VerifyError as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, ensure_ascii=False, indent=2))
        return 2
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    return rc


if __name__ == "__main__":
    raise SystemExit(main())
