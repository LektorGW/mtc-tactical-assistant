"""Package the tested Windows release without including development caches."""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import zipfile
from pathlib import Path


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--window-report", required=True, type=Path)
    parser.add_argument("--allow-unverified-physical-hotkeys", action="store_true",
                        help="Package with an explicit limitation when physical tests were interrupted")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    version = json.loads((root / "apps/desktop/package.json").read_text("utf-8"))["version"]
    release = root / "release"
    exe = release / f"MTC-Tactical-Assistant-{version}.exe"
    installer = release / f"MTC Tactical Assistant_{version}_x64-setup.exe"
    smoke_path = root / f"tests/output/v{version}/smoke-test.json"
    smoke = json.loads(smoke_path.read_text("utf-8"))
    window_path = args.window_report.resolve()
    windows = json.loads(window_path.read_text("utf-8"))
    if not smoke.get("passed") or smoke.get("exeSha256") != digest(exe):
        raise SystemExit("Smoke test must pass for this exact release executable")
    extra_reports = []
    if tuple(map(int, version.split('.'))) >= (0, 3, 0):
        reports=['automation-smoke.json','map-hud-smoke.json']
        if tuple(map(int,version.split('.'))) >= (0,4,0): reports+=['resources-dynamics-smoke.json','marker-picker-smoke.json']
        if tuple(map(int,version.split('.'))) >= (0,5,0): reports+=['live-hud-smoke.json']
        for name in reports:
            report_path = smoke_path.parent / name
            report = json.loads(report_path.read_text('utf-8'))
            incomplete = report.get('preliminary') or report.get('pending') or any(isinstance(item, dict) and item.get('status') != 'passed' for item in report.get('checks', []))
            if not report.get('passed') or report.get('exeSha256') != digest(exe) or incomplete:
                raise SystemExit(f'{name} must pass without pending checks for this exact executable')
            extra_reports.append(report_path)
    if Path(windows["executable"]).resolve() != exe.resolve():
        raise SystemExit("Window report belongs to a different executable")
    hotkeys = windows.get("hotkeys", [])
    physical_verified = bool(hotkeys) and all(item.get("passed") for item in hotkeys)
    if not physical_verified and not args.allow_unverified_physical_hotkeys:
        raise SystemExit("Physical shortcut checks are incomplete or failed")
    if not windows.get("nativeStateChecks") or any(not item.get("passed") for item in windows["nativeStateChecks"]):
        raise SystemExit("Native window state checks are incomplete or failed")
    payload = windows.get("installer", {})
    if (not payload.get("communityLicenseIncluded")
            or not payload.get("payloadMatchesExceptTauriBundleMarker")
            or payload.get("executable", {}).get("sha256") != digest(exe)
            or payload.get("installer", {}).get("sha256") != digest(installer)):
        raise SystemExit("Installer audit must match the release and include the source license")

    # Explicit file list: old releases, WebView caches and test settings stay outside ZIP.
    files: dict[str, Path] = {
        exe.name: exe,
        installer.name: installer,
        "README.md": root / "README.md",
        "licenses/mtc-community-MIT.txt": root / "data/weapons/community/LICENSE.md",
        "licenses/community-manifest.json": root / "data/weapons/community/manifest.json",
        "verification/smoke-test.json": smoke_path,
        "verification/window-controls-report.json": window_path,
        "verification/wgc-clock-probe-summary.json": root / "tests/output/wgc-clock-probe-summary.json",
    }
    files.update({f"docs/{p.name}": p for p in sorted((root / "docs").glob("*.md"))})
    files.update({f"verification/{p.name}": p for p in extra_reports})
    for name in ('automation-fit.png', 'automation-3d.png', 'map-hud-automatic.png', 'unit-test-summary.json', 'resources-panel.png', 'dynamics-panel.png', 'marker-picker.png', 'assets-summary.json', 'assets-verification.json', 'assets-offline.json', 'normal-resources-setup.json', 'live-aim-panel.png'):
        path = smoke_path.parent / name
        if path.exists():
            files[f'verification/{name}'] = path
    for name in ("batch-analysis.json", "desktop-map.png", "desktop-ocr.png", "desktop-automatic.png", "overlay.png"):
        path = smoke_path.parent / name
        if path.exists():
            files[f"verification/{name}"] = path
    for name, source in files.items():
        destination = release / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.resolve() != source.resolve():
            shutil.copyfile(source, destination)
    sums_path = release / f"SHA256SUMS-{version}.txt"
    sums_path.write_text("\n".join(f"{digest(p).upper()}  {p.name}" for p in (exe, installer)) + "\n", "utf-8")
    files[sums_path.name] = sums_path
    archive = release / f"MTC-Tactical-Assistant-{version}-Windows-x64.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as bundle:
        for name, path in files.items():
            bundle.write(path, name)
    with zipfile.ZipFile(archive) as bundle:
        if bundle.testzip() is not None:
            raise SystemExit("Archive CRC validation failed")
    metadata = {"version": version, "zip": archive.name, "zipSha256": digest(archive),
                "exeSha256": digest(exe), "installerSha256": digest(installer),
                "smokeChecks": len(smoke["checks"]), "physicalKeys": len(hotkeys),
                "physicalHotkeysVerified": physical_verified,
                "automationChecks": sum(len(json.loads(p.read_text('utf-8'))['checks']) for p in extra_reports),
                "files": sorted(files)}
    (release / f"release-{version}.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2), "utf-8")
    print(json.dumps({k:v for k,v in metadata.items() if k != "files"}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
