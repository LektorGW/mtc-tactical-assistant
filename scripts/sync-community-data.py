"""Import a pinned, MIT-licensed MTC community snapshot without executing source code.

Python 3.10+, standard library only. All imported values are game values; this
tool does not run upstream Luau export scripts or connect to a game process.
"""
from __future__ import annotations

import argparse
import ast
import datetime as dt
import hashlib
import json
import math
from pathlib import Path
import re
import urllib.request

REPOSITORY = "https://github.com/grand-hawk/artillery-calculator"
PINNED_COMMIT = "39a7af7689524e57ca2326f3b998cf123df95426"
ROOT = Path(__file__).resolve().parents[1]
CONFIG = "packages/mtc-artillery/src/config/"
FILES = {
    "LICENSE.md": "data/weapons/community/LICENSE.md",
    "README.md": "data/weapons/community/source/README.md",
    CONFIG + "importedGuns.json": "data/weapons/community/source/importedGuns.json",
    CONFIG + "guns.ts": "data/weapons/community/source/guns.ts",
    CONFIG + "maps.ts": "data/maps/community/source/maps.ts",
    "packages/mtc-artillery/src/utils/math.ts": "data/weapons/community/source/math.ts",
}


def download(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "MTC-Tactical-Assistant-community-import"})
    with urllib.request.urlopen(request, timeout=30) as response:
        data = response.read(4_000_001)
    if len(data) > 4_000_000:
        raise ValueError("Source exceeds the import size limit")
    return data


def object_literal(text: str) -> object:
    """Convert the upstream's restricted config object literals, never eval JS."""
    text = re.sub(r"^\s*//.*$", "", text, flags=re.M)
    text = re.sub(r'''"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*' ''', lambda m: json.dumps(ast.literal_eval(m.group(0).strip()), ensure_ascii=False), text, flags=re.X)
    text = re.sub(r"(^|[{,]\s*)([A-Za-z_][A-Za-z0-9_]*|[0-9]+)\s*:", r'\1"\2":', text)
    text = re.sub(r",\s*([}\]])", r"\1", text)
    return json.loads(text)


def parse_custom_guns(text: str) -> dict:
    match = re.search(r"const customGuns[^=]*=\s*({.*?})\s*;", text, flags=re.S)
    if not match:
        raise ValueError("Upstream customGuns shape changed; manual review required")
    return object_literal(match.group(1))


def parse_maps(text: str) -> dict:
    match = re.search(r"export const gameMaps[^=]*=\s*({.*?})\s*satisfies", text, flags=re.S)
    if not match:
        raise ValueError("Upstream gameMaps shape changed; manual review required")
    literal = match.group(1)
    # Upstream uses only numeric literals, integer * integer and this conversion.
    literal = re.sub(r"calculateMapSize\((\d+(?:\.\d+)?)\)", lambda m: str(float(m.group(1)) * (5 / 1.8) * 9), literal)
    literal = re.sub(r"(\d+(?:\.\d+)?)\s*\*\s*(\d+(?:\.\d+)?)", lambda m: str(float(m.group(1)) * float(m.group(2))), literal)
    return object_literal(literal)


def category(key: str) -> str:
    # Category is an application grouping inferred from names, not source mechanics.
    if key == "_9K720_Iskander":
        return "guided_missile"
    if key == "ub32":
        return "helicopter_rocket"
    if key in {"RBU_6000_MT_LB", "M26_T99", "LUAZ_672", "T34_Calliope", "RSZO_1", "RSZO_2", "M270_MLRS", "TOS_1A_BM_1", "M17_Whizbang", "BM_21_Grad", "RBT_5", "Pz_W_42", "Sturmtiger"}:
        return "rocket_artillery"
    if key in {"mortar", "d30", "vasilek", "hellcannon", "pounder", "M7_Priest", "SAU_2", "_2S43", "Karl_Ger__t", "PzH_2000", "_2S7_Pion", "M109", "M109A6", "_2S19_Msta_S", "ags17"}:
        return "tube_artillery"
    return "tank"


def write_json(path: str, value: object) -> None:
    destination = ROOT / path
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--commit", default=PINNED_COMMIT, help="Full reviewed commit SHA")
    parser.add_argument("--refresh", action="store_true", help="Resolve dev HEAD; review output before distributing")
    parser.add_argument("--offline", action="store_true", help="Regenerate from bundled raw source files")
    args = parser.parse_args()
    if args.offline and args.refresh:
        parser.error("--offline and --refresh cannot be combined")
    commit = args.commit
    if args.refresh:
        commit = json.loads(download("https://api.github.com/repos/grand-hawk/artillery-calculator/commits/dev"))["sha"]
    if not re.fullmatch(r"[a-f0-9]{40}", commit):
        parser.error("A complete 40-character lowercase commit SHA is required")
    source = {}
    manifest_files = []
    for remote, local in FILES.items():
        path = ROOT / local
        url = f"https://raw.githubusercontent.com/grand-hawk/artillery-calculator/{commit}/{remote}"
        data = path.read_bytes() if args.offline else download(url)
        source[remote] = data.decode("utf-8-sig")
        manifest_files.append({"path": local, "sourceUrl": url, "sha256": hashlib.sha256(data).hexdigest()})
        if not args.offline:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if "MIT License" not in source["LICENSE.md"] or "Copyright (c) 2024 Astrid" not in source["LICENSE.md"]:
        raise ValueError("License changed: stop and review before importing")
    math_source = source["packages/mtc-artillery/src/utils/math.ts"]
    if "const G = 9.8 * 1.8;" not in math_source or "return s / (5 / 1.8);" not in math_source:
        raise ValueError("Game gravity/unit convention changed: manual review required")
    manifest_path = ROOT / "data/weapons/community/manifest.json"
    if args.offline:
        prior = json.loads(manifest_path.read_text(encoding="utf-8"))
        if prior["commit"] != commit:
            raise ValueError("Offline commit does not match saved source provenance")
        expected_files = {f["path"]: f["sha256"] for f in prior["files"]}
        if any(expected_files.get(f["path"]) != f["sha256"] for f in manifest_files):
            raise ValueError("Offline source checksum differs; review local source changes")
        fetched_at = prior["fetchedAt"]
    else:
        fetched_at = dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")
    metadata = {
        "repository": REPOSITORY,
        "commit": commit,
        "fetchedAt": fetched_at,
        "license": "MIT",
        "experienceId": "9520328730",
        "urls": [f"{REPOSITORY}/blob/{commit}/{p}" for p in [CONFIG + "importedGuns.json", CONFIG + "guns.ts", "packages/mtc-artillery/src/utils/math.ts", "README.md"]],
    }
    guns = {**parse_custom_guns(source[CONFIG + "guns.ts"]), **json.loads(source[CONFIG + "importedGuns.json"])}
    profiles = []
    for key, gun in sorted(guns.items(), key=lambda item: item[1]["name"].casefold()):
        for index, projectile in enumerate(gun["projectiles"]):
            velocity = projectile["velocity"]
            if not isinstance(velocity, (int, float)) or not math.isfinite(velocity) or velocity <= 0:
                raise ValueError(f"Invalid game velocity: {key}")
            profiles.append({
                "schemaVersion": 1,
                "id": f"community-{key.lower().strip('_')}-{index + 1}",
                "name": f"{gun['name']} / {projectile['name'].strip()}",
                "category": category(key),
                "gameVersion": "community snapshot; experience 9520328730; zgodność aktualnej gry niepotwierdzona",
                "status": "experimental",
                "source": f"{REPOSITORY}@{commit}; MIT Copyright (c) 2024 Astrid",
                "calibrated": False,
                "speed": velocity,
                "gravity": 9.8 * 1.8,
                "minElevationDeg": -89,
                "maxElevationDeg": 89,
                "dispersionSigma": None,
                "salvoSize": 1,
                "notes": "Parametry kalkulatora społeczności MTC; brak niezależnej walidacji w aktualnej grze. Model bez oporu/napędu. Limity ±89° to zakres solwera, nie potwierdzone limity pojazdu. SalvoSize=1 oznacza pojedynczy pocisk, nie pojemność wyrzutni. Kategoria to grupowanie nazw."
                + (" Naprowadzanie nieweryfikowane; solwer wyłączony." if category(key) == "guided_missile" else ""),
                "unit": "game_meters",
                "sourceMetadata": metadata,
                "sourceAcknowledged": False,
            })
    # Hand-entered single values from other cited sources (never bulk imports).
    # They keep their own provenance/status and are re-applied on every import.
    curated_path = ROOT / "data/weapons/community/curated-parameters.json"
    curated = json.loads(curated_path.read_text(encoding="utf-8")) if curated_path.exists() else {"profiles": {}}
    by_id = {profile["id"]: profile for profile in profiles}
    for profile_id, patch in curated["profiles"].items():
        if profile_id not in by_id or set(patch) - {"salvo"}:
            raise ValueError(f"Curated parameter does not match the snapshot: {profile_id}")
        salvo = patch["salvo"]
        if salvo.get("calibrated") or salvo.get("status") != "experimental" or not salvo.get("source", "").strip():
            raise ValueError(f"Curated salvo data must stay experimental with a source: {profile_id}")
        by_id[profile_id].update(patch)
    maps = []
    for key, game_map in sorted(parse_maps(source[CONFIG + "maps.ts"]).items()):
        size = game_map["size"]
        if not isinstance(size, (int, float)) or not math.isfinite(size) or size <= 0:
            raise ValueError(f"Invalid map size: {key}")
        heights = game_map.get("heightmap")
        maps.append({
            "schemaVersion": 1, "id": key, "name": game_map["name"], "key": game_map["key"],
            "sizeStuds": size, "sizeGameMeters": size * 0.36, "unit": "game_meters",
            "status": "experimental", "inRotation": game_map.get("inRotation", False),
            "heightRangeStuds": {"minimum": heights["0"], "maximum": heights["255"]} if heights else None,
            "sourceMetadata": {**metadata, "urls": [f"{REPOSITORY}/blob/{commit}/{CONFIG}maps.ts", f"{REPOSITORY}/blob/{commit}/packages/mtc-artillery/src/utils/math.ts"]},
        })
    write_json("data/weapons/community-profiles.json", profiles)
    write_json("data/maps/community-maps.json", maps)
    write_json("data/weapons/community/manifest.json", {
        "schemaVersion": 1, "repository": REPOSITORY, "commit": commit, "fetchedAt": fetched_at,
        "license": "MIT", "profileCount": len(profiles), "weaponCount": len(guns), "mapCount": len(maps),
        "files": manifest_files,
    })
    print(f"Imported {len(profiles)} projectile profiles from {len(guns)} weapons and {len(maps)} maps at {commit}")


if __name__ == "__main__":
    main()
