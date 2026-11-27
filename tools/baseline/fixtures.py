#!/usr/bin/env python3
"""The baseline's test projects, made the same way every time (fixed seeds).

    python3 tools/baseline/fixtures.py <out-dir>

Writes three projects into <out-dir>, using `ppstudio build` (crates/ppstudio-cli):
- empty.ppstudio: 1920x1080, one layer, one empty cel.
- layers20-4k.ppstudio: 3840x2160, 20 layers, a 24-frame cut. A paper layer and 15
  still layers of line work are held for the whole cut, and 4 layers animate on twos
  (64 drawings).
- drawings500.ppstudio: 1920x1080, 5 layers of 100 drawings each, a 100-frame cut.

The drawings are line work with some flat colour, like a real cel: mostly transparent,
so they compress like real drawings do. Needs Pillow.
"""

import json
import math
import random
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw

INKS = [(22, 22, 26), (60, 52, 70), (40, 40, 48)]
FLATS = [(232, 73, 47), (246, 196, 83), (90, 150, 210), (120, 190, 140), (210, 160, 200)]


def drawing(path: Path, w: int, h: int, seed: int, strokes: int, fills: int) -> None:
    """A drawing: a few flat-colour shapes under wobbly ink lines."""
    if path.exists():
        return
    rnd = random.Random(seed)
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    g = ImageDraw.Draw(img)
    s = w / 1920
    for _ in range(fills):
        cx, cy = rnd.uniform(0.1, 0.9) * w, rnd.uniform(0.1, 0.9) * h
        r = rnd.uniform(60, 220) * s
        pts = [
            (cx + r * rnd.uniform(0.7, 1.2) * math.cos(a), cy + r * rnd.uniform(0.7, 1.2) * math.sin(a))
            for a in [i * math.tau / 9 for i in range(9)]
        ]
        g.polygon(pts, fill=rnd.choice(FLATS) + (255,))
    for _ in range(strokes):
        x, y = rnd.uniform(0.05, 0.95) * w, rnd.uniform(0.05, 0.95) * h
        a = rnd.uniform(0, math.tau)
        pts = [(x, y)]
        for _ in range(rnd.randint(12, 40)):
            a += rnd.uniform(-0.35, 0.35)
            step = rnd.uniform(8, 22) * s
            x, y = x + step * math.cos(a), y + step * math.sin(a)
            pts.append((x, y))
        g.line(pts, fill=rnd.choice(INKS) + (255,), width=max(2, int(rnd.uniform(2, 6) * s)), joint="curve")
    img.save(path, optimize=False, compress_level=1)


def paper(path: Path, w: int, h: int) -> None:
    if not path.exists():
        Image.new("RGBA", (w, h), (246, 244, 239, 255)).save(path, compress_level=1)


def build(spec: dict, name: str, out: Path) -> None:
    spec_path = out / "pics" / f"{name}.json"
    spec_path.write_text(json.dumps(spec))
    target = out / f"{name}.ppstudio"
    subprocess.run(
        ["cargo", "run", "-q", "--release", "-p", "pencilpilot-ppstudio-cli", "--", "build", str(spec_path), str(target)],
        check=True,
        stdout=subprocess.DEVNULL,
    )
    print(f"{target}  {target.stat().st_size / 1e6:.1f} MB")


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = Path(sys.argv[1]).resolve()
    pics = out / "pics"
    pics.mkdir(parents=True, exist_ok=True)

    # empty
    build(
        {
            "width": 1920,
            "height": 1080,
            "layers": [{"name": "Layer 1"}],
            "scenes": [{"name": "Scene 1", "cuts": [{"name": "Cut 1", "tracks": {"Layer 1": [{"length": 1}]}}]}],
        },
        "empty",
        out,
    )

    # 20 layers at 4K
    w, h = 3840, 2160
    paper(pics / "paper-4k.png", w, h)
    layers = [{"name": "Paper", "static": True}]
    tracks = {"Paper": [{"image": "paper-4k.png", "length": 24}]}
    for i in range(15):
        n = f"Still {i + 1}"
        f = f"still4k-{i}.png"
        drawing(pics / f, w, h, 1000 + i, strokes=10, fills=2)
        layers.append({"name": n})
        tracks[n] = [{"image": f, "length": 24}]
    for i in range(4):
        n = f"Anim {i + 1}"
        layers.append({"name": n})
        tracks[n] = []
        for k in range(12):
            f = f"anim4k-{i}-{k}.png"
            drawing(pics / f, w, h, 2000 + i * 100 + k, strokes=8, fills=1)
            tracks[n].append({"image": f, "length": 2, "kind": "key" if k % 4 == 0 else None})
    build(
        {
            "width": w,
            "height": h,
            "fps": 24,
            "layers": layers,
            "scenes": [{"name": "Scene 1", "cuts": [{"name": "Cut 1", "tracks": tracks}]}],
        },
        "layers20-4k",
        out,
    )

    # 500 drawings
    w, h = 1920, 1080
    layers, tracks = [], {}
    for i in range(5):
        n = f"Layer {i + 1}"
        layers.append({"name": n})
        tracks[n] = []
        for k in range(100):
            f = f"d500-{i}-{k}.png"
            drawing(pics / f, w, h, 3000 + i * 1000 + k, strokes=6, fills=1 if i % 2 else 0)
            tracks[n].append({"image": f, "length": 1})
    build(
        {
            "width": w,
            "height": h,
            "fps": 24,
            "layers": layers,
            "scenes": [{"name": "Scene 1", "cuts": [{"name": "Cut 1", "tracks": tracks}]}],
        },
        "drawings500",
        out,
    )


if __name__ == "__main__":
    main()
