"""Generate original synthetic map/terrain assets; never captures a game frame."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct
import zlib


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))


def write_grayscale_png(path: Path, width: int, height: int, samples: bytes) -> dict:
    if len(samples) != width * height:
        raise ValueError("Wrong sample count")
    rows = b"".join(b"\0" + samples[y * width:(y + 1) * width] for y in range(height))
    content = (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(rows, level=9))
        + png_chunk(b"IEND", b"")
    )
    path.write_bytes(content)
    return {"path": str(path.resolve()), "sha256": hashlib.sha256(content).hexdigest(), "width": width, "height": height}


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=root / "tests/output/v0.3.0/fixtures")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    width, height = 350, 200
    samples = bytes(40 + (((y // 20) * 37 + (x // 25) * 61 + (y // 20) * (x // 25) * 7) % 170)
                    for y in range(height) for x in range(width))
    reference = write_grayscale_png(output / "reference-map.png", width, height, samples)
    terrain = write_grayscale_png(output / "heightmap.png", 2, 2, bytes([0, 85, 170, 255]))
    manifest = {
        "schemaVersion": 1,
        "source": "SYNTHETIC: original deterministic fixture, not an MTC map or measured terrain",
        "reference": reference,
        "terrain": terrain,
        "fixture": {"arguments": ["--auto-hud", "--passive", "--seconds", "120"],
                    "mapClientBounds": {"x": 270, "y": 240, "width": width, "height": height},
                    "ownReferenceCenter": {"x": 46, "y": 36}, "targetReferenceCenter": {"x": 236, "y": 136},
                    "timer": 1662, "heading": 138, "vehicle": "BM-21 Grad", "ammunition": "9M22/M21 HE-Frag"},
        "terrainEncoding": {"minimum": 0, "maximum": 100, "normalizedSamples": [0, 1/3, 2/3, 1],
                            "expectedCenterHeight": 50,
                            "expectedMarkerHeightDelta": 100 * (190 / 349 / 3 + 2 * 100 / 199 / 3)},
    }
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, ensure_ascii=True))


if __name__ == "__main__":
    main()
