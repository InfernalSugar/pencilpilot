use super::*;

const W: usize = 150;
const H: usize = 40;

fn canvas(f: impl Fn(usize, usize) -> Option<[u8; 4]>) -> Vec<u8> {
    let mut a = vec![0u8; W * H * 4];
    for y in 0..H {
        for x in 0..W {
            if let Some(p) = f(x, y) {
                a[(y * W + x) * 4..][..4].copy_from_slice(&p);
            }
        }
    }
    a
}

fn layer(name: &str, blend: &str, opacity: f64, visible: bool, clip: bool) -> PsdLayer {
    PsdLayer {
        name: name.into(),
        opacity,
        blend: blend.into(),
        visible,
        clip,
    }
}

fn noise(x: usize, y: usize) -> u8 {
    ((x as i32).wrapping_mul(73856093) ^ (y as i32).wrapping_mul(19349663)) as u8
}

/// The document `tests/psd/golden.mjs` builds, the same way.
fn fixture() -> PsdDoc {
    let mask: Vec<u8> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if x < 75 { 255 } else { (y * 6) as u8 }
        })
        .collect();
    let unknown = format!("Unknown {}", "é".repeat(260));
    PsdDoc {
        width: W as u32,
        height: H as u32,
        composite: canvas(|x, y| Some([(x * 3 + y) as u8, (x ^ y) as u8, (x * y) as u8, 255])),
        items: vec![
            PsdItem::Layer(layer("Paper", "normal", 1.0, true, false)),
            PsdItem::Layer(layer("Ink ✏️ é", "multiply", 0.7, true, false)),
            PsdItem::Layer(layer("Shade", "screen", 0.333, true, true)),
            PsdItem::Group {
                name: "Folder".into(),
                opacity: 0.5,
                visible: false,
                children: vec![
                    layer("Masked", "overlay", 1.0, true, false),
                    layer("Empty", "lighten", 1.0, true, false),
                ],
            },
            PsdItem::Layer(layer("Top add", "add", 0.9, false, false)),
            PsdItem::Layer(layer(&unknown, "hue", 1.0, true, false)),
        ],
        pixels: vec![
            (canvas(|_, _| Some([240, 235, 220, 255])), None),
            (
                canvas(|x, y| {
                    ((10..90).contains(&x) && (5..30).contains(&y)).then(|| {
                        [
                            (x * 7) as u8,
                            (y * 11) as u8,
                            ((x + y) * 5) as u8,
                            if (x + y) % 3 == 0 { 128 } else { 255 },
                        ]
                    })
                }),
                None,
            ),
            (
                canvas(|x, y| {
                    (y >= 20).then(|| {
                        let n = noise(x, y);
                        [n, n.wrapping_mul(3), n.wrapping_mul(7), n | 1]
                    })
                }),
                None,
            ),
            (
                canvas(|x, _| {
                    Some([
                        (((x >> 1) & 1) * 200) as u8,
                        50,
                        (((x >> 2) & 1) * 100) as u8,
                        255,
                    ])
                }),
                Some(mask),
            ),
            (canvas(|_, _| None), None),
            (
                canvas(|x, y| (x == 149 && y == 39).then_some([1, 2, 3, 4])),
                None,
            ),
            (
                canvas(|x, y| (y == 0).then_some([x as u8, x as u8, x as u8, x as u8])),
                None,
            ),
        ],
    }
}

#[test]
fn the_same_bytes_as_the_current_apps_writer() {
    let ours = fixture().to_bytes();
    let theirs = include_bytes!("../../tests/psd/ts.psd");
    assert_eq!(ours.len(), theirs.len(), "lengths");
    if let Some(i) = ours.iter().zip(theirs.iter()).position(|(a, b)| a != b) {
        panic!("first difference at byte {i}");
    }
}

// ---- reading it back (as `packages/paint/src/psd-read.ts` reads files) ----

struct Read<'a> {
    b: &'a [u8],
    p: usize,
}

impl Read<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        s
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_be_bytes(self.take(2).try_into().unwrap())
    }
    fn i16(&mut self) -> i16 {
        self.u16() as i16
    }
    fn u32(&mut self) -> u32 {
        u32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
}

/// PackBits rows back to bytes.
fn unpack(src: &[u8], counts: &[usize], cols: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(counts.len() * cols);
    let mut p = 0;
    for &n in counts {
        let end = p + n;
        let start = out.len();
        while p < end {
            let h = src[p];
            p += 1;
            if h < 128 {
                out.extend_from_slice(&src[p..p + h as usize + 1]);
                p += h as usize + 1;
            } else if h > 128 {
                let v = src[p];
                p += 1;
                out.extend(std::iter::repeat_n(v, 257 - h as usize));
            }
        }
        assert_eq!(out.len() - start, cols, "a row's length");
    }
    out
}

fn decode_block(b: &[u8], cols: usize, rows: usize) -> Vec<u8> {
    match u16::from_be_bytes([b[0], b[1]]) {
        0 => {
            assert_eq!(b.len() - 2, cols * rows);
            b[2..].to_vec()
        }
        1 => {
            let counts: Vec<usize> = (0..rows)
                .map(|y| u16::from_be_bytes([b[2 + y * 2], b[3 + y * 2]]) as usize)
                .collect();
            let data = &b[2 + rows * 2..];
            assert_eq!(counts.iter().sum::<usize>(), data.len());
            unpack(data, &counts, cols)
        }
        c => panic!("compression {c}"),
    }
}

#[derive(Debug)]
struct Back {
    name: String,
    blend: [u8; 4],
    opacity: u8,
    clip: bool,
    hidden: bool,
    section: Option<u32>,
    rgba: Vec<u8>,
    mask: Option<Vec<u8>>,
}

fn read(b: &[u8]) -> (usize, usize, Vec<Back>, Vec<u8>) {
    let mut r = Read { b, p: 0 };
    assert_eq!(r.take(4), b"8BPS");
    assert_eq!(r.u16(), 1);
    r.take(6);
    assert_eq!(r.u16(), 3);
    let h = r.u32() as usize;
    let w = r.u32() as usize;
    assert_eq!(r.u16(), 8);
    assert_eq!(r.u16(), 3);
    let n = r.u32() as usize;
    r.take(n);
    let n = r.u32() as usize;
    r.take(n);
    let lm = r.u32() as usize;
    let lm_end = r.p + lm;
    let li = r.u32() as usize;
    let li_end = r.p + li;
    let count = r.i16().unsigned_abs() as usize;
    struct Rec {
        rect: [i32; 4],
        chans: Vec<(i16, usize)>,
        back: Back,
        mask_rect: Option<[i32; 4]>,
    }
    let mut recs = Vec::new();
    for _ in 0..count {
        let rect = [r.i32(), r.i32(), r.i32(), r.i32()];
        let nc = r.u16() as usize;
        let chans: Vec<(i16, usize)> = (0..nc).map(|_| (r.i16(), r.u32() as usize)).collect();
        assert_eq!(r.take(4), b"8BIM");
        let blend: [u8; 4] = r.take(4).try_into().unwrap();
        let opacity = r.u8();
        let clip = r.u8() == 1;
        let flags = r.u8();
        r.u8();
        let extra = r.u32() as usize;
        let end = r.p + extra;
        let ml = r.u32() as usize;
        let mask_rect = (ml > 0).then(|| {
            let m = [r.i32(), r.i32(), r.i32(), r.i32()];
            r.take(ml - 16);
            m
        });
        let br = r.u32() as usize;
        r.take(br);
        let pl = r.u8() as usize;
        r.take(pl);
        let mut k = pl + 1;
        while !k.is_multiple_of(4) {
            r.u8();
            k += 1;
        }
        let (mut name, mut section) = (String::new(), None);
        while r.p < end {
            assert_eq!(r.take(4), b"8BIM");
            let key: [u8; 4] = r.take(4).try_into().unwrap();
            let len = r.u32() as usize;
            let stop = r.p + len;
            match &key {
                b"luni" => {
                    let n = r.u32() as usize;
                    let units: Vec<u16> = (0..n).map(|_| r.u16()).collect();
                    name = String::from_utf16_lossy(&units);
                }
                b"lsct" => section = Some(r.u32()),
                _ => {}
            }
            r.p = stop;
        }
        assert_eq!(r.p, end, "a record's extra data");
        recs.push(Rec {
            rect,
            chans,
            mask_rect,
            back: Back {
                name,
                blend,
                opacity,
                clip,
                hidden: flags & 2 != 0,
                section,
                rgba: vec![0; w * h * 4],
                mask: None,
            },
        });
    }
    for rec in &mut recs {
        let [t, l, bm, rt] = rec.rect;
        let (cw, ch) = ((rt - l) as usize, (bm - t) as usize);
        for &(id, len) in &rec.chans {
            let block = r.take(len).to_vec();
            if id == -2 {
                let [mt, ml, mb, mr] = rec.mask_rect.expect("a mask's rectangle");
                let (mw, mh) = ((mr - ml) as usize, (mb - mt) as usize);
                rec.back.mask = Some(decode_block(&block, mw, mh));
                continue;
            }
            let plane = decode_block(&block, cw, ch);
            let c = if id == -1 { 3 } else { id as usize };
            for y in 0..ch {
                for x in 0..cw {
                    rec.back.rgba[((y + t as usize) * w + x + l as usize) * 4 + c] =
                        plane[y * cw + x];
                }
            }
        }
    }
    assert!(
        r.p <= li_end && li_end - r.p <= 1,
        "the layer info's padding"
    );
    r.p = li_end;
    assert_eq!(r.u32(), 0, "no global mask");
    assert_eq!(r.p, lm_end);
    assert_eq!(r.u16(), 1);
    let counts: Vec<usize> = (0..h * 3).map(|_| r.u16() as usize).collect();
    let flat = unpack(&b[r.p..], &counts, w);
    assert_eq!(r.p + counts.iter().sum::<usize>(), b.len());
    let mut composite = vec![255u8; w * h * 4];
    for c in 0..3 {
        for i in 0..w * h {
            composite[i * 4 + c] = flat[c * w * h + i];
        }
    }
    (w, h, recs.into_iter().map(|r| r.back).collect(), composite)
}

#[test]
fn it_reads_back_as_it_was_written() {
    let doc = fixture();
    let (w, h, back, composite) = read(&doc.to_bytes());
    assert_eq!((w, h), (W, H));
    assert_eq!(composite, doc.composite);
    // bottom → top: the folder's end, its layers, then the folder
    let names: Vec<&str> = back.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        &names[..7],
        [
            "Paper",
            "Ink ✏️ é",
            "Shade",
            "</Layer group>",
            "Masked",
            "Empty",
            "Folder"
        ]
    );
    assert_eq!(back[3].section, Some(3));
    assert_eq!(back[6].section, Some(1));
    assert_eq!(&back[6].blend, b"pass");
    assert!(back[6].hidden);
    assert_eq!(back[6].opacity, 128);
    // names cut to 255 UTF-16 units
    assert_eq!(back[8].name.encode_utf16().count(), 255);
    let layers: Vec<&Back> = back.iter().filter(|b| b.section.is_none()).collect();
    assert_eq!(layers.len(), doc.pixels.len());
    let all: Vec<&PsdLayer> = doc
        .items
        .iter()
        .flat_map(|i| match i {
            PsdItem::Layer(l) => vec![l],
            PsdItem::Group { children, .. } => children.iter().collect(),
        })
        .collect();
    for ((b, l), (rgba, mask)) in layers.iter().zip(&all).zip(&doc.pixels) {
        assert_eq!(&b.rgba, rgba, "{}", l.name);
        assert_eq!(&b.mask, mask, "{}", l.name);
        assert_eq!(b.clip, l.clip);
        assert_eq!(b.hidden, !l.visible);
        assert_eq!(b.opacity, (l.opacity * 255.0).round() as u8);
        assert_eq!(&b.blend, blend_key(&l.blend));
    }
}

#[test]
fn rows_never_grow_and_pairs_stay_literal() {
    // alternating pairs, noise, runs at the 128 boundary
    let mut rows: Vec<Vec<u8>> = vec![
        (0..300).map(|i| (i / 2 % 2) as u8 * 255).collect(),
        (0..300).map(|i| noise(i, 7)).collect(),
        vec![9; 129],
        vec![9; 130],
        (0..256)
            .map(|i| if i < 128 { 1 } else { i as u8 })
            .collect(),
    ];
    rows.push(vec![]);
    for row in rows {
        let mut out = Vec::new();
        pack_row(&row, &mut out);
        assert!(out.len() <= row.len() + row.len().div_ceil(128));
        if !row.is_empty() {
            assert_eq!(unpack(&out, &[out.len()], row.len()), row);
        }
    }
}

#[test]
fn a_big_mostly_empty_layer_streams_small() {
    // 2000 × 1500 with a small stroke: the file stays far below its raw size, and each
    // layer's pixels are asked for twice
    let (w, h) = (2000usize, 1500usize);
    let mut asked = 0;
    let mut out = Vec::new();
    let comp = vec![255u8; w * h * 4];
    write_psd(
        &mut out,
        w as u32,
        h as u32,
        &comp,
        &[PsdItem::Layer(layer("Stroke", "normal", 1.0, true, false))],
        &mut |_| {
            asked += 1;
            let mut a = vec![0u8; w * h * 4];
            for x in 500..900 {
                a[(700 * w + x) * 4..][..4].copy_from_slice(&[10, 20, 30, 255]);
            }
            (a, None)
        },
    )
    .unwrap();
    assert_eq!(asked, 2);
    assert!(out.len() < 200_000, "{}", out.len());
}
