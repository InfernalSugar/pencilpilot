#!/usr/bin/env python3
"""Checks an export against reference frames (`ppstudio render <project> <dir> --every`: one
PNG a frame, over white), and measures a process tree's memory.

    python3 -I export_check.py video  <file.mp4|webm> <ref-dir>     decoded back with ffmpeg
    python3 -I export_check.py gif    <file.gif> <ref-dir>
    python3 -I export_check.py pngseq <dir> <ref-dir>                 pixels compared exactly
    python3 -I export_check.py treemem <pid> [interval-s]             PSS of a pid and its
                                                                      descendants until it exits

Prints JSON. Video and GIF: the stream's codec, size, frame rate, frame count (counted by
decoding), duration; per frame the PSNR (dB, RGB) and the mean absolute difference (levels)
against the reference frame of the same number. A reference of another size is scaled to the
export's with a box filter first (`scaledRef`). Needs ffmpeg/ffprobe, numpy and Pillow.
"""

import glob
import json
import math
import os
import subprocess
import sys
import time

import numpy as np
from PIL import Image


def refs(ref_dir):
    return sorted(glob.glob(os.path.join(ref_dir, "*.png")))


def probe(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries",
         "stream=codec_name,pix_fmt,width,height,r_frame_rate,avg_frame_rate,nb_read_frames,duration"
         ":format=duration,format_name", "-of", "json", path],
        capture_output=True, text=True, check=True).stdout
    j = json.loads(out)
    s = j["streams"][0]
    num, den = (int(x) for x in s["r_frame_rate"].split("/"))
    return {
        "format": j["format"].get("format_name"),
        "codec": s.get("codec_name"), "pixFmt": s.get("pix_fmt"),
        "width": s["width"], "height": s["height"],
        "fps": round(num / den, 3) if den else None,
        "avgFps": s.get("avg_frame_rate"),
        "frames": int(s.get("nb_read_frames", 0)),
        "duration": float(s.get("duration") or j["format"].get("duration") or 0),
    }


def decode(path, w, h):
    """Each decoded frame, RGB uint8 (h, w, 3), in order (no frame rate conversion)."""
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", path, "-fps_mode", "passthrough",
                          "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], stdout=subprocess.PIPE)
    n = w * h * 3
    while True:
        b = p.stdout.read(n)
        if len(b) < n:
            break
        yield np.frombuffer(b, np.uint8).reshape(h, w, 3)
    p.wait()


def ref_rgb(path, w, h):
    im = Image.open(path).convert("RGB")
    scaled = im.size != (w, h)
    if scaled:
        im = im.resize((w, h), Image.BOX)
    return np.asarray(im), scaled


def compare(a, b):
    d = a.astype(np.int16) - b.astype(np.int16)
    mse = float(np.mean(d.astype(np.float64) ** 2))
    psnr = 99.0 if mse == 0 else 10 * math.log10(255 * 255 / mse)
    return round(psnr, 2), round(float(np.mean(np.abs(d))), 3), int(np.max(np.abs(d)))


def check_stream(path, ref_dir):
    info = probe(path)
    rs = refs(ref_dir)
    per = []
    scaled = False
    for i, fr in enumerate(decode(path, info["width"], info["height"])):
        if i >= len(rs):
            per.append({"frame": i + 1, "ref": None})
            continue
        r, scaled = ref_rgb(rs[i], info["width"], info["height"])
        psnr, mad, mx = compare(fr, r)
        per.append({"frame": i + 1, "psnr": psnr, "mad": mad, "max": mx})
    got = [p for p in per if p.get("psnr") is not None]
    info.update({
        "refFrames": len(rs), "decodedFrames": len(per), "scaledRef": scaled,
        "psnrMin": min((p["psnr"] for p in got), default=None),
        "psnrMean": round(sum(p["psnr"] for p in got) / len(got), 2) if got else None,
        "madMax": max((p["mad"] for p in got), default=None),
        "madMean": round(sum(p["mad"] for p in got) / len(got), 3) if got else None,
        "worst": min(got, key=lambda p: p["psnr"]) if got else None,
        "perFrame": per,
    })
    return info


def check_gif(path, ref_dir):
    im = Image.open(path)
    n, durs = 0, []
    while True:
        durs.append(im.info.get("duration"))
        n += 1
        try:
            im.seek(im.tell() + 1)
        except EOFError:
            break
    info = check_stream(path, ref_dir)
    info.update({"pilFrames": n, "frameMs": sorted(set(durs)), "totalMs": sum(d or 0 for d in durs),
                 "loop": im.info.get("loop")})
    return info


def check_pngseq(d, ref_dir):
    got = sorted(glob.glob(os.path.join(d, "*.png")))
    rs = refs(ref_dir)
    per = []
    for i, (g, r) in enumerate(zip(got, rs)):
        a = np.asarray(Image.open(g).convert("RGBA"))
        b = np.asarray(Image.open(r).convert("RGBA"))
        if a.shape != b.shape:
            per.append({"frame": i + 1, "size": [a.shape[1], a.shape[0]], "refSize": [b.shape[1], b.shape[0]]})
            continue
        # an exported frame with transparency is compared as it shows over white
        if a[..., 3].min() < 255:
            al = a[..., 3:4].astype(np.float64) / 255
            a = np.dstack([np.round(a[..., :3] * al + 255 * (1 - al)).astype(np.uint8), np.full(a.shape[:2], 255, np.uint8)])
        diff = np.abs(a.astype(np.int16) - b.astype(np.int16)).max(axis=2)
        per.append({"frame": i + 1, "max": int(diff.max()), "differing": int((diff > 0).sum()),
                    "over1": int((diff > 1).sum())})
    return {"files": len(got), "refFrames": len(rs), "names": [os.path.basename(g) for g in got[:3]],
            "exact": all(p.get("max") == 0 for p in per) and len(got) == len(rs),
            "maxDiff": max((p.get("max", 255) for p in per), default=None),
            "framesDiffering": sum(1 for p in per if p.get("max", 1) > 0), "perFrame": per}


def tree(pid):
    """The pid and its descendants."""
    kids = {}
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        try:
            with open(f"/proc/{p}/stat") as f:
                ppid = int(f.read().rsplit(")", 1)[1].split()[1])
            kids.setdefault(ppid, []).append(int(p))
        except (OSError, ValueError, IndexError):
            pass
    out, todo = [], [pid]
    while todo:
        p = todo.pop()
        out.append(p)
        todo.extend(kids.get(p, []))
    return out


def pss_kib(pid):
    try:
        with open(f"/proc/{pid}/smaps_rollup") as f:
            for line in f:
                if line.startswith("Pss:"):
                    return int(line.split()[1])
    except OSError:
        return 0
    return 0


def comm(pid):
    try:
        with open(f"/proc/{pid}/comm") as f:
            return f.read().strip()
    except OSError:
        return "?"


def treemem(pid, interval=0.05):
    """Samples until the pid exits (or stdin closes). Peak of the sum, and the peak per
    program name."""
    peak, peak_by, samples = 0, {}, []
    t0 = time.monotonic()
    while os.path.exists(f"/proc/{pid}"):
        ps = tree(pid)
        by = {}
        for p in ps:
            by[comm(p)] = by.get(comm(p), 0) + pss_kib(p)
        tot = sum(by.values())
        samples.append([round(time.monotonic() - t0, 3), tot])
        if tot > peak:
            peak, peak_by = tot, by
        time.sleep(interval)
    return {"peakMiB": round(peak / 1024, 1), "peakBy": {k: round(v / 1024, 1) for k, v in peak_by.items()},
            "samples": len(samples), "seconds": samples[-1][0] if samples else 0}


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    kind = sys.argv[1]
    if kind == "video":
        r = check_stream(sys.argv[2], sys.argv[3])
    elif kind == "gif":
        r = check_gif(sys.argv[2], sys.argv[3])
    elif kind == "pngseq":
        r = check_pngseq(sys.argv[2], sys.argv[3])
    elif kind == "treemem":
        r = treemem(int(sys.argv[2]), float(sys.argv[3]) if len(sys.argv) > 3 else 0.05)
    else:
        sys.exit(__doc__)
    if "--brief" in sys.argv:
        r.pop("perFrame", None)
    print(json.dumps(r))


if __name__ == "__main__":
    main()
