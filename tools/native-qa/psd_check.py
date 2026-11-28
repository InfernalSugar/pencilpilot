#!/usr/bin/env python3
"""A PSD from the native app against today's app's writer, byte for byte, and read back.

    python3 -I psd_check.py <native.psd> <out-dir> [--psd-ts <packages/paint/src/psd.ts>]
                            [--composite <the CPU's picture, raw RGBA>]

1. The native PSD is read here (a reader of its own, written from the format: header,
   layer records with their masks, names (Unicode `luni`), folders (`lsct`), channel data
   raw or PackBits, the flattened picture), into the document today's writer takes
   (`writePsd`'s `PsdDoc`: canvas-sized straight RGBA per layer, grey masks, groups, the
   composite).
2. That document goes through today's writer (`psd_ts.mjs`, node), and the two files are
   compared byte for byte; the first difference is shown with where it is (which record,
   which channel).
3. What was read is summarised (layers, groups, blend modes, opacity, clipping, masks,
   bounds) for checks against the project.

Writes <out-dir>/psd-check.json.
"""

import json
import os
import struct
import subprocess
import sys

BLENDS = {b"norm": "normal", b"mul ": "multiply", b"scrn": "screen", b"over": "overlay",
          b"dark": "darken", b"lite": "lighten", b"lddg": "add", b"pass": "pass"}
HERE = os.path.dirname(os.path.realpath(__file__))


class R:
    def __init__(self, b, at=0):
        self.b, self.at, self.marks = b, at, []

    def take(self, n):
        v = self.b[self.at:self.at + n]
        if len(v) != n:
            raise ValueError(f"short read at {self.at}")
        self.at += n
        return v

    def u8(self):
        return self.take(1)[0]

    def u16(self):
        return struct.unpack(">H", self.take(2))[0]

    def i16(self):
        return struct.unpack(">h", self.take(2))[0]

    def u32(self):
        return struct.unpack(">I", self.take(4))[0]

    def i32(self):
        return struct.unpack(">i", self.take(4))[0]


def unpack_bits(data, n):
    out, i = bytearray(), 0
    while len(out) < n and i < len(data):
        c = data[i]
        i += 1
        if c < 128:
            out += data[i:i + c + 1]
            i += c + 1
        elif c > 128:
            out += bytes([data[i]]) * (257 - c)
            i += 1
    if len(out) != n:
        raise ValueError(f"PackBits row: {len(out)} bytes, want {n}")
    return bytes(out)


def channel(r, cols, rows, length):
    end = r.at + length
    comp = r.u16()
    if comp == 0:
        data = r.take(cols * rows)
    elif comp == 1:
        counts = [r.u16() for _ in range(rows)]
        data = b"".join(unpack_bits(r.take(c), cols) for c in counts)
    else:
        raise ValueError(f"compression {comp}")
    if r.at != end:
        raise ValueError(f"channel ends at {r.at}, its length says {end}")
    return comp, data


def read_psd(b):
    r = R(b)
    assert r.take(4) == b"8BPS", "not a PSD"
    hdr = {"version": r.u16()}
    r.take(6)
    hdr.update(channels=r.u16(), height=r.u32(), width=r.u32(), depth=r.u16(), mode=r.u16())
    w, h = hdr["width"], hdr["height"]
    r.take(r.u32())  # colour mode data
    r.take(r.u32())  # image resources
    lm_len = r.u32()
    lm_end = r.at + lm_len
    li_len = r.u32()
    li_end = r.at + li_len
    count = r.i16()
    recs = []
    for _ in range(abs(count)):
        rec = {"at": r.at}
        rec["top"], rec["left"], rec["bottom"], rec["right"] = r.i32(), r.i32(), r.i32(), r.i32()
        rec["channels"] = [(r.i16(), r.u32()) for _ in range(r.u16())]
        assert r.take(4) == b"8BIM"
        key = r.take(4)
        rec["blend"] = BLENDS.get(key, key.decode("latin1"))
        rec["opacity"], rec["clip"], rec["flags"] = r.u8(), r.u8(), r.u8()
        r.u8()
        ex_end = r.u32()
        ex_end += r.at
        ml = r.u32()
        rec["mask"] = None
        if ml:
            m = R(r.take(ml))
            rec["mask"] = {"top": m.i32(), "left": m.i32(), "bottom": m.i32(), "right": m.i32(),
                           "default": m.u8(), "flags": m.u8()}
        r.take(r.u32())  # blending ranges
        n = r.u8()
        rec["pascal"] = r.take(n).decode("latin1")
        r.take((4 - (n + 1) % 4) % 4)
        rec["extra"] = []
        while r.at < ex_end:
            sig, k = r.take(4), r.take(4).decode("latin1")
            data = r.take(r.u32())
            rec["extra"].append(k)
            if k == "luni":
                ln = struct.unpack(">I", data[:4])[0]
                rec["name"] = data[4:4 + 2 * ln].decode("utf-16-be")
            elif k == "lsct":
                rec["section"] = struct.unpack(">I", data[:4])[0]
        assert r.at == ex_end, f"record extra ends at {r.at}, want {ex_end}"
        rec.setdefault("name", rec["pascal"])
        recs.append(rec)
    for rec in recs:
        rec["data"] = {}
        rec["comp"] = {}
        for cid, ln in rec["channels"]:
            at = r.at
            if cid == -2 and rec["mask"]:
                m = rec["mask"]
                cols, rows = m["right"] - m["left"], m["bottom"] - m["top"]
            else:
                cols, rows = rec["right"] - rec["left"], rec["bottom"] - rec["top"]
            comp, data = channel(r, cols, rows, ln)
            rec["data"][cid] = data
            rec["comp"][cid] = (comp, at)
    # (a pad byte to an even length)
    if r.at < li_end:
        r.take(li_end - r.at)
    r.at = lm_end
    comp = r.u16()
    if comp == 1:
        counts = [r.u16() for _ in range(h * 3)]
        planes = b"".join(unpack_bits(r.take(c), w) for c in counts)
    else:
        planes = r.take(w * h * 3)
    assert r.at == len(b), f"{len(b) - r.at} bytes after the picture"
    comp_rgba = bytearray(w * h * 4)
    for c in range(3):
        comp_rgba[c::4] = planes[c * w * h:(c + 1) * w * h]
    comp_rgba[3::4] = b"\xff" * (w * h)
    return hdr, recs, bytes(comp_rgba)


def canvas_rgba(rec, w, h):
    out = bytearray(w * h * 4)
    rw, rh = rec["right"] - rec["left"], rec["bottom"] - rec["top"]
    for ci, cid in enumerate((0, 1, 2, -1)):
        d = rec["data"].get(cid)
        if not d or not rw:
            continue
        for y in range(rh):
            row = d[y * rw:(y + 1) * rw]
            at = ((y + rec["top"]) * w + rec["left"]) * 4 + ci
            out[at:at + rw * 4:4] = row
    return bytes(out)


def to_doc(hdr, recs, comp, d):
    """The writer's document from what was read; its pixel files into d."""
    w, h = hdr["width"], hdr["height"]
    os.makedirs(d, exist_ok=True)
    open(os.path.join(d, "composite.raw"), "wb").write(comp)
    items, group = [], None
    for i, rec in enumerate(recs):
        sec = rec.get("section")
        if sec == 3:
            group = []
            continue
        if sec in (1, 2):
            items.append({"kind": "group", "name": rec["name"], "opacity": rec["opacity"] / 255,
                          "visible": not rec["flags"] & 2, "children": group or []})
            group = None
            continue
        f = f"layer-{i}.raw"
        open(os.path.join(d, f), "wb").write(canvas_rgba(rec, w, h))
        layer = {"kind": "layer", "name": rec["name"], "rgba": f, "opacity": rec["opacity"] / 255,
                 "blend": rec["blend"], "visible": not rec["flags"] & 2, "clip": bool(rec["clip"])}
        if rec["mask"]:
            mf = f"mask-{i}.raw"
            open(os.path.join(d, mf), "wb").write(rec["data"][-2])
            layer["mask"] = mf
        (group if group is not None else items).append(layer)
    doc = {"width": w, "height": h, "composite": "composite.raw", "items": items}
    json.dump(doc, open(os.path.join(d, "doc.json"), "w"), indent=1)
    return doc


def where(recs, at):
    for i, rec in enumerate(recs):
        for cid, (comp, start) in rec.get("comp", {}).items():
            ln = dict(rec["channels"])[cid]
            if start <= at < start + ln:
                return f"record {i} ({rec['name']!r}) channel {cid}, compression {comp}, byte {at - start}"
    for i, rec in enumerate(recs):
        nxt = recs[i + 1]["at"] if i + 1 < len(recs) else None
        if rec["at"] <= at and (nxt is None or at < nxt) and at < recs[-1]["at"] + 400:
            return f"in record {i} ({rec['name']!r})'s header"
    return "outside the layer records (header, section lengths or the flattened picture)"


def main():
    a = sys.argv[1:]
    if len(a) < 2:
        sys.exit(__doc__)
    psd, out = a[0], a[1]
    ts = a[a.index("--psd-ts") + 1] if "--psd-ts" in a else os.path.join(
        HERE, "../../packages/paint/src/psd.ts")
    os.makedirs(out, exist_ok=True)
    b = open(psd, "rb").read()
    hdr, recs, comp = read_psd(b)
    doc = to_doc(hdr, recs, comp, os.path.join(out, "doc"))
    ts_out = os.path.join(out, "ts.psd")
    subprocess.run(["node", os.path.join(HERE, "psd_ts.mjs"), os.path.realpath(ts),
                    os.path.join(out, "doc", "doc.json"), ts_out], check=True, stdout=subprocess.DEVNULL)
    t = open(ts_out, "rb").read()
    first = next((i for i, (x, y) in enumerate(zip(b, t)) if x != y), None)
    if first is None and len(b) != len(t):
        first = min(len(b), len(t))
    res = {
        "native": psd, "bytes": len(b), "tsBytes": len(t), "identical": b == t,
        "firstDifference": first, "where": None if first is None else where(recs, first),
        "nativeAround": None if first is None else b[max(0, first - 8):first + 8].hex(),
        "tsAround": None if first is None else t[max(0, first - 8):first + 8].hex(),
        "header": hdr,
        "layers": [{"name": r["name"], "blend": r["blend"], "opacity": r["opacity"], "clip": r["clip"],
                    "hidden": bool(r["flags"] & 2), "section": r.get("section"),
                    "rect": [r["top"], r["left"], r["bottom"], r["right"]], "mask": r["mask"],
                    "extra": r["extra"], "compression": {str(k): v[0] for k, v in r["comp"].items()}}
                   for r in recs],
        "items": [{k: v for k, v in i.items() if k in ("kind", "name")} for i in doc["items"]],
    }
    if "--composite" in a:
        # the flattened picture against the CPU's picture of the frame (RGB, alpha left out)
        want = open(a[a.index("--composite") + 1], "rb").read()
        diffs = [abs(x - y) for i, (x, y) in enumerate(zip(comp, want)) if i % 4 != 3]
        res["compositeVsCpu"] = {"maxDiff": max(diffs) if diffs else None,
                                 "over1": sum(1 for v in diffs if v > 1), "sameSize": len(comp) == len(want)}
    json.dump(res, open(os.path.join(out, "psd-check.json"), "w"), indent=1)
    print(json.dumps({k: res.get(k) for k in ("bytes", "tsBytes", "identical", "firstDifference", "where",
                                              "compositeVsCpu")}))


if __name__ == "__main__":
    main()
