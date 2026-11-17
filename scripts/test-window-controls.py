"""Read-only Windows window/PE inspector and isolated test-process lifecycle.

No SendInput, SendKeys, registry writes, DPI settings, game process access,
or installer execution. All keyboard actions belong to @oai/sky in the JS test.
"""
from __future__ import annotations

import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
user32 = c.WinDLL("user32", use_last_error=True)
kernel32 = c.WinDLL("kernel32", use_last_error=True)
user32.GetForegroundWindow.restype = w.HWND
user32.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
user32.GetWindowLongPtrW.argtypes = [w.HWND, c.c_int]
user32.GetWindowLongPtrW.restype = c.c_ssize_t
user32.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
user32.GetWindowRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
user32.IsWindowVisible.argtypes = [w.HWND]
user32.IsIconic.argtypes = [w.HWND]
user32.GetWindowTextLengthW.argtypes = [w.HWND]
user32.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
user32.GetDpiForWindow.argtypes = [w.HWND]
user32.SetThreadDpiAwarenessContext.argtypes = [c.c_void_p]
user32.SetThreadDpiAwarenessContext.restype = c.c_void_p
kernel32.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
kernel32.OpenProcess.restype = w.HANDLE
kernel32.QueryFullProcessImageNameW.argtypes = [w.HANDLE, w.DWORD, w.LPWSTR, c.POINTER(w.DWORD)]
kernel32.CloseHandle.argtypes = [w.HANDLE]
kernel32.TerminateProcess.argtypes = [w.HANDLE, w.UINT]


def process_path(pid: int) -> str | None:
    handle = kernel32.OpenProcess(0x1000, False, pid)
    if not handle:
        return None
    try:
        size = w.DWORD(32768)
        path = c.create_unicode_buffer(size.value)
        return path.value if kernel32.QueryFullProcessImageNameW(handle, 0, path, c.byref(size)) else None
    finally:
        kernel32.CloseHandle(handle)


def inspect(pid: int) -> dict:
    # Only this short-lived inspector thread changes awareness, never Windows
    # scaling or the tested application. Avoid GetWindowRect DPI virtualization.
    previous_awareness = user32.SetThreadDpiAwarenessContext(c.c_void_p(-4))
    foreground = user32.GetForegroundWindow()
    foreground_pid = w.DWORD()
    user32.GetWindowThreadProcessId(foreground, c.byref(foreground_pid))
    windows = []
    callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)

    def callback(handle, _):
        owner = w.DWORD()
        user32.GetWindowThreadProcessId(handle, c.byref(owner))
        if owner.value != pid:
            return True
        title = c.create_unicode_buffer(user32.GetWindowTextLengthW(handle) + 1)
        user32.GetWindowTextW(handle, title, len(title))
        if title.value not in {"MTC Tactical Assistant", "MTC Tactical Overlay", "MTC HUD fixture"}:
            return True
        frame, client = w.RECT(), w.RECT()
        user32.GetWindowRect(handle, c.byref(frame))
        user32.GetClientRect(handle, c.byref(client))
        style = user32.GetWindowLongPtrW(handle, -16) & 0xFFFFFFFF
        extended = user32.GetWindowLongPtrW(handle, -20) & 0xFFFFFFFF
        windows.append({
            "handle": int(handle), "pid": owner.value, "title": title.value,
            "visible": bool(user32.IsWindowVisible(handle)), "minimized": bool(user32.IsIconic(handle)),
            "foreground": handle == foreground, "dpi": user32.GetDpiForWindow(handle),
            "frame": {"x": frame.left, "y": frame.top, "width": frame.right - frame.left, "height": frame.bottom - frame.top},
            "client": {"width": client.right - client.left, "height": client.bottom - client.top},
            "style": hex(style), "extendedStyle": hex(extended),
            "flags": {"topmost": bool(extended & 0x8), "transparentInput": bool(extended & 0x20), "layered": bool(extended & 0x80000), "toolWindow": bool(extended & 0x80), "caption": bool(style & 0xC00000)},
        })
        return True

    callback_pointer = callback_type(callback)
    try:
        user32.EnumWindows(callback_pointer, 0)
    finally:
        if previous_awareness:
            user32.SetThreadDpiAwarenessContext(previous_awareness)
    return {"pid": pid, "path": process_path(pid), "coordinateSpace": "physical_pixels_inspector_per_monitor_v2" if previous_awareness else "default_thread_awareness", "foregroundHandle": int(foreground or 0), "foregroundPid": foreground_pid.value, "windows": windows}


def scoped_output(path: str) -> Path:
    output = Path(path).resolve()
    output.relative_to(ROOT / "tests")
    output.mkdir(parents=True, exist_ok=True)
    return output


def pe(path: Path) -> dict:
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise ValueError("Not a Windows PE file")
    offset = struct.unpack_from("<I", data, 0x3C)[0]
    if data[offset:offset + 4] != b"PE\0\0":
        raise ValueError("Invalid PE signature")
    machine = struct.unpack_from("<H", data, offset + 4)[0]
    optional_magic = struct.unpack_from("<H", data, offset + 24)[0]
    return {"path": str(path), "size": len(data), "sha256": hashlib.sha256(data).hexdigest(), "machine": hex(machine), "pe32Plus": optional_magic == 0x20B}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    info = commands.add_parser("inspect")
    info.add_argument("--pid", type=int, required=True)
    launch = commands.add_parser("launch")
    launch.add_argument("--exe", required=True)
    launch.add_argument("--output-dir", required=True)
    launch.add_argument("--port", type=int, default=9224)
    stop = commands.add_parser("stop")
    stop.add_argument("--pid", required=True, type=int)
    stop.add_argument("--exe", required=True)
    payload = commands.add_parser("payload")
    payload.add_argument("--installer", required=True)
    payload.add_argument("--exe", required=True)
    payload.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    if args.command == "inspect":
        result = inspect(args.pid)
    elif args.command == "launch":
        output = scoped_output(args.output_dir)
        executable = Path(args.exe).resolve(strict=True)
        environment = os.environ.copy()
        environment["WEBVIEW2_USER_DATA_FOLDER"] = str(output / "webview")
        environment["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={args.port}"
        process = subprocess.Popen([str(executable), "--test-data-dir", str(output / "settings")], env=environment, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        result = {"pid": process.pid, "exe": str(executable), "outputDir": str(output), "port": args.port}
    elif args.command == "stop":
        expected = str(Path(args.exe).resolve(strict=True)).casefold()
        actual = process_path(args.pid)
        if not actual or actual.casefold() != expected:
            raise ValueError("Refusing to stop a process outside this exact test executable")
        handle = kernel32.OpenProcess(0x1, False, args.pid)
        if not handle:
            raise c.WinError(c.get_last_error())
        try:
            if not kernel32.TerminateProcess(handle, 0):
                raise c.WinError(c.get_last_error())
        finally:
            kernel32.CloseHandle(handle)
        result = {"stoppedPid": args.pid}
    else:
        output = scoped_output(args.output_dir)
        installer = Path(args.installer).resolve(strict=True)
        executable = Path(args.exe).resolve(strict=True)
        seven_zip = shutil.which("7z")
        if not seven_zip:
            raise ValueError("7z/NanaZip is required for read-only NSIS payload inspection")
        extracted = output / "installer-payload"
        extracted.mkdir(exist_ok=True)
        extraction = subprocess.run([seven_zip, "x", "-y", f"-o{extracted}", str(installer)], capture_output=True, text=True, encoding="utf-8", errors="replace")
        if extraction.returncode:
            raise ValueError("Payload extraction failed: " + extraction.stderr)
        nsi = extracted / "[NSIS].nsi"
        script = nsi.read_text(encoding="utf-8", errors="replace") if nsi.exists() else ""
        registry_lines = [line.strip() for line in script.splitlines() if any(token.lower() in line.lower() for token in ["WriteReg", "DeleteReg", "Uninstall", "HKCU", "HKLM"])]
        app_payload = extracted / "mtc-tactical-assistant.exe"
        actual_exe, packed_exe = pe(executable), pe(app_payload)
        # Tauri patches this fixed-width marker when preparing an NSIS bundle.
        portable_bytes, bundled_bytes = executable.read_bytes(), app_payload.read_bytes()
        normalize = lambda data: data.replace(b"BUNDLE_TYPE_VAR_UNK", b"BUNDLE_TYPE_VAR_XXX").replace(b"BUNDLE_TYPE_VAR_NSS", b"BUNDLE_TYPE_VAR_XXX")
        license_files = []
        for file in extracted.rglob("*"):
            if file.is_file() and file.suffix.lower() in {".txt", ".md"} and file.stat().st_size < 256_000:
                text = file.read_text(encoding="utf-8-sig", errors="replace")
                if "MIT License" in text and "Copyright (c) 2024 Astrid" in text:
                    license_files.append({"path": str(file.relative_to(extracted)), "sha256": hashlib.sha256(file.read_bytes()).hexdigest()})
        result = {"installer": pe(installer), "executable": actual_exe, "payloadExecutable": packed_exe,
                  "payloadMatchesRelease": actual_exe["sha256"] == packed_exe["sha256"],
                  "payloadMatchesExceptTauriBundleMarker": normalize(portable_bytes) == normalize(bundled_bytes),
                  "communityLicenseIncluded": bool(license_files), "communityLicenseFiles": license_files,
                  "installerExecuted": False, "registryMutationDetected": any("writereg" in line.lower() or "deletereg" in line.lower() for line in registry_lines),
                  "registryEvidence": registry_lines[:30], "extractedFiles": [str(p.relative_to(extracted)) for p in extracted.rglob("*") if p.is_file()]}
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(json.dumps({"error": str(error)}, ensure_ascii=False), file=sys.stderr)
        sys.exit(1)
