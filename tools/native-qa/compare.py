#!/usr/bin/env python3
"""Design vs native: regions, colours and corners, measured from the pixels of both.

    python3 -I tools/native-qa/compare.py <design-dir> <native-dir> <out-dir>

<design-dir> holds design-<ws>.png and design-<ws>.json (from render-design.mjs);
<native-dir> holds native-<ws>.png (from `pencilpilot --render <ws>`). For each workspace it
writes <out-dir>/<ws>-side.png (design | native | difference), <out-dir>/<ws>-diff.png, and
all measurements to <out-dir>/compare.json and <out-dir>/compare.md.

How regions are measured (the same way on both images, so the detector is checked against the
design's own DOM boxes too): from a seed point in the region's padding, 4 px inside its left
edge, flood-fill the seed's colour; the region is the filled area's bounding box. The corner radius comes from the diagonal: the paper run from the box's corner
along the diagonal is r * (1 - 1/sqrt 2).
"""

import json
import math
import sys
from collections import Counter
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops, ImageDraw

WORKSPACES = ["animate", "draw", "colour", "review"]
PAPER = (0xF6, 0xF4, 0xEF)
# tolerances
POS_TOL = 2.0  # px, position and size
COLOUR_TOL = 6  # max channel difference
RADIUS_TOL = 4.0  # px
PAPER_TOL = 2
SENTINEL = (1, 2, 3)


def pixels(img):
    """An image's pixels, in order (Pillow renamed getdata)."""
    f = getattr(img, "get_flattened_data", None)
    return f() if f else img.getdata()


def is_paper(p):
    return all(abs(a - b) <= PAPER_TOL for a, b in zip(p[:3], PAPER))


def measure_region(img, seed):
    """The box of the region under `seed`: the bounding box of the area of the seed's colour
    connected to it (flood fill, ±8 a channel). Content drawn on the region doesn't matter, as
    long as the region's own colour surrounds it."""
    px = img.load()
    sx, sy = int(seed[0]), int(seed[1])
    if is_paper(px[sx, sy]):
        return None
    work = img.copy()
    ImageDraw.floodfill(work, (sx, sy), SENTINEL, thresh=8)
    mask = (np.asarray(work) == SENTINEL).all(-1)
    ys, xs = np.nonzero(mask)
    box = {
        "x": int(xs.min()),
        "y": int(ys.min()),
        "w": int(xs.max() - xs.min() + 1),
        "h": int(ys.max() - ys.min() + 1),
    }
    box["radius"] = corner_radius(img, box)
    box["colour"] = hexc(px[sx, sy])
    return box


def corner_radius(img, box):
    """From the top-left corner along the diagonal: paper pixels before the shape."""
    px = img.load()
    k = 0
    while k < min(box["w"], box["h"]) // 2 and is_paper(px[box["x"] + k, box["y"] + k]):
        k += 1
    return round(k / (1 - 1 / math.sqrt(2)), 1)


def dominant(img, box, n=1):
    """The most common colour(s) in the box (as #RRGGBB)."""
    crop = img.crop((box["x"], box["y"], box["x"] + box["w"], box["y"] + box["h"]))
    crop = crop.resize((max(1, crop.width // 2), max(1, crop.height // 2)), Image.NEAREST)
    c = Counter(pixels(crop))
    top = [hexc(col) for col, _ in c.most_common(n)]
    return top[0] if n == 1 else top


def palette(img, box, min_share=0.004):
    """Colours covering at least `min_share` of the box."""
    crop = img.crop((box["x"], box["y"], box["x"] + box["w"], box["y"] + box["h"]))
    c = Counter(pixels(crop))
    total = sum(c.values())
    return {hexc(col): round(n / total, 4) for col, n in c.most_common(24) if n / total >= min_share}


def hexc(p):
    return "#%02X%02X%02X" % tuple(p[:3])


def rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i : i + 2], 16) for i in (0, 2, 4))


def css_hex(css):
    """'rgb(22, 22, 26)' -> '#16161A' (None for transparent)."""
    if not css or "rgba(0, 0, 0, 0)" in css:
        return None
    nums = [float(v) for v in css[css.index("(") + 1 : css.index(")")].split(",")]
    return "#%02X%02X%02X" % tuple(round(v) for v in nums[:3])


def colour_close(a, b):
    return a is not None and b is not None and max(abs(x - y) for x, y in zip(rgb(a), rgb(b))) <= COLOUR_TOL


def content_bbox(img, y0, y1):
    """The bounding box of non-paper pixels in rows y0..y1 (the header has no box of its own)."""
    px = img.load()
    w = img.size[0]
    xs, ys = [], []
    for y in range(y0, y1):
        for x in range(w):
            if not is_paper(px[x, y]):
                xs.append(x)
                ys.append(y)
    if not xs:
        return None
    return {"x": min(xs), "y": min(ys), "w": max(xs) - min(xs) + 1, "h": max(ys) - min(ys) + 1}


def seed_for(name, box):
    # in the region's padding, clear of its content (and of the design's frame menu)
    return (box["x"] + 4, box["y"] + box["h"] * 0.5)


def side_by_side(design, native, diff, out):
    w, h = design.size
    pad = 40
    canvas = Image.new("RGB", (w * 3 + 40, h + pad), (255, 255, 255))
    d = ImageDraw.Draw(canvas)
    for i, (label, im) in enumerate([("design", design), ("native", native), ("difference", diff)]):
        canvas.paste(im, (i * (w + 20), pad))
        d.text((i * (w + 20) + 8, 12), label, fill=(0, 0, 0))
    canvas.save(out)


def main():
    if len(sys.argv) != 4:
        print(__doc__)
        sys.exit(2)
    ddir, ndir, out = (Path(a) for a in sys.argv[1:])
    out.mkdir(parents=True, exist_ok=True)
    report = {}
    failures = 0
    lines = ["# Design vs native: measured", ""]
    for ws in WORKSPACES:
        dpng, djson, npng = ddir / f"design-{ws}.png", ddir / f"design-{ws}.json", ndir / f"native-{ws}.png"
        if not (dpng.exists() and djson.exists() and npng.exists()):
            lines.append(f"## {ws}: missing input, skipped\n")
            continue
        design = Image.open(dpng).convert("RGB")
        native = Image.open(npng).convert("RGB")
        meta = json.loads(djson.read_text())
        if native.size != design.size:
            native_cmp = native.resize(design.size)
            size_note = f"native is {native.size}, design {design.size}: resized to compare"
        else:
            native_cmp = native
            size_note = None
        diff = ImageChops.difference(design, native_cmp)
        # amplify so small differences show
        diff_vis = diff.point(lambda v: min(255, v * 4))
        diff_vis.save(out / f"{ws}-diff.png")
        side_by_side(design, native_cmp, diff_vis, out / f"{ws}-side.png")
        differing = sum(1 for p in pixels(diff) if max(p) > 24)
        rows = []
        ws_report = {"sizeNote": size_note, "pixelsDiffering": differing, "regions": {}}
        lines.append(f"## {ws}")
        lines.append("")
        lines.append(
            f"Images: `{ws}-side.png`, `{ws}-diff.png`. Pixels differing by more than 24 in a channel: "
            f"{differing} of {design.width * design.height} ({100 * differing / (design.width * design.height):.1f}%)."
        )
        lines.append("")
        lines.append("| Region | Design DOM (x, y, w, h · colour · radius) | Design pixels | Native pixels | Verdict |")
        lines.append("|---|---|---|---|---|")
        for name in ["tools", "canvas", "panel", "strip"]:
            dbox = (meta.get("regions") or {}).get(name)
            if dbox is None:
                # the design has no such region here: native must not have one either
                if name == "tools":
                    nb = measure_region(native_cmp, (20, 450))
                    has = nb is not None and nb["w"] < 120
                    verdict = "FAIL: native shows a tool column the design doesn't" if has else "ok (none in either)"
                    failures += has
                    rows.append((name, "none", "none", fmt(nb) if has else "none", verdict))
                    ws_report["regions"][name] = {"design": None, "native": nb if has else None, "ok": not has}
                continue
            seed = seed_for(name, dbox)
            dpix = measure_region(design, seed)
            npix = measure_region(native_cmp, seed)
            want_col = css_hex(dbox.get("bg"))
            problems = []
            if npix is None:
                problems.append(f"nothing at the design's {name} (paper at {tuple(round(v) for v in seed)})")
            else:
                for k in ("x", "y", "w", "h"):
                    if abs(npix[k] - dbox[k]) > POS_TOL:
                        problems.append(f"{k} {npix[k]} vs {round(dbox[k], 1)} ({npix[k] - dbox[k]:+.0f})")
                if want_col and not colour_close(npix["colour"], want_col):
                    problems.append(f"colour {npix['colour']} vs {want_col}")
                if abs(npix["radius"] - dbox["radius"]) > RADIUS_TOL:
                    problems.append(f"radius ~{npix['radius']} vs {dbox['radius']}")
            ok = not problems
            failures += not ok
            ddesc = f"{r1(dbox)} · {want_col} · {dbox['radius']}"
            rows.append((name, ddesc, fmt(dpix), fmt(npix), "ok" if ok else "FAIL: " + "; ".join(problems)))
            ws_report["regions"][name] = {"design": dbox, "designPixels": dpix, "native": npix, "problems": problems}
        # the header: no box of its own; compare what's drawn above the body
        body_top = int((meta["regions"].get("canvas") or {}).get("y", 84))
        ncanvas = ws_report["regions"].get("canvas", {}).get("native")
        top = min(body_top, ncanvas["y"]) if ncanvas else body_top
        dh = content_bbox(design, 0, top - 1)
        nh = content_bbox(native_cmp, 0, top - 1)
        hdr = meta["regions"]["header"]
        problems = []
        if nh is None:
            problems.append("nothing drawn above the body")
        else:
            for k in ("x", "y", "w", "h"):
                if dh and abs(nh[k] - dh[k]) > POS_TOL:
                    problems.append(f"content {k} {nh[k]} vs {dh[k]} ({nh[k] - dh[k]:+d})")
        failures += bool(problems)
        rows.append(
            (
                "header (content above the body)",
                f"box {r1(hdr)}",
                fmt(dh),
                fmt(nh),
                "ok" if not problems else "FAIL: " + "; ".join(problems),
            )
        )
        ws_report["regions"]["header"] = {"design": hdr, "designPixels": dh, "native": nh, "problems": problems}
        for r in rows:
            lines.append("| " + " | ".join(r) + " |")
        lines.append("")
        # colours: each region's palette, and design colours the native region lacks
        lines.append("Key colours the design region uses (at least 2%% of it) that the native region lacks (±%d):" % COLOUR_TOL)
        lines.append("")
        pal_report = {}
        for name in ["header", "tools", "canvas", "panel", "strip"]:
            dbox = meta["regions"].get(name)
            if not dbox:
                continue
            if name == "header":
                box = {"x": 16, "y": 16, "w": 1408, "h": body_top - 28}
            else:
                box = {k: int(round(dbox[k])) for k in ("x", "y", "w", "h")}
            dp = palette(design, box, 0.02)
            np_ = palette(native_cmp, box, 0.004)
            missing = [c for c in dp if not any(colour_close(c, n) for n in np_)]
            pal_report[name] = {"design": dp, "native": np_, "missing": missing}
            lines.append(f"- **{name}**: design {fmt_pal(dp)}; native {fmt_pal(np_)}; missing: {', '.join(missing) or 'none'}")
        lines.append("")
        ws_report["palettes"] = pal_report
        # element counts the design shows
        lines.append(
            "Design elements: "
            f"{len(meta.get('workspaceTabs', []))} workspace tabs ({', '.join(t['label'] for t in meta.get('workspaceTabs', []))}); "
            f"{len(meta.get('toolButtons', []))} tool-column buttons ({', '.join(t['label'] for t in meta.get('toolButtons', []))}); "
            f"panel tabs: {', '.join(t['label'] for t in meta.get('panelTabs', [])) or 'none'}; "
            f"header buttons: {', '.join(t['label'] for t in meta.get('headerButtons', []))}."
        )
        lines.append("")
        report[ws] = ws_report
    (out / "compare.json").write_text(json.dumps(report, indent=2))
    (out / "compare.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    print(f"\n{failures} region check(s) failed")
    sys.exit(1 if failures else 0)


def r1(b):
    return f"{b['x']:.0f}, {b['y']:.0f}, {b['w']:.0f}, {b['h']:.0f}"


def fmt(b):
    if not b:
        return "—"
    extra = ""
    if "colour" in b:
        extra = f" · {b['colour']} · r~{b['radius']}"
    return r1(b) + extra


def fmt_pal(p):
    return " ".join(f"{c} {v * 100:.0f}%" for c, v in list(p.items())[:6])


if __name__ == "__main__":
    main()
