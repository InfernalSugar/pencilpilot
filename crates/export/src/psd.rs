//! A Photoshop (.psd) writer: 8-bit RGB, layers with names, opacity, blend modes, visibility,
//! clipping, layer masks and groups, plus the flattened picture; PackBits where it's smaller.
//! The current app's writer (`packages/paint/src/psd.ts`), ported to give the same bytes for
//! the same document.
//!
//! Layers' pixels are asked for one layer at a time, twice (once to size the file's sections,
//! once to write them), so a document of many big layers is never all in memory at once.

use std::io::{self, Write};

/// A layer as the file keeps it (its pixels come from [`write_psd`]'s `pixels`).
#[derive(Debug, Clone, PartialEq)]
pub struct PsdLayer {
    pub name: String,
    /// 0–1 (as the current app has it: a double)
    pub opacity: f64,
    /// the app's blend name ("normal", "multiply"…); others are written as normal
    pub blend: String,
    pub visible: bool,
    /// clipped to the layer below
    pub clip: bool,
}

/// What a document holds, bottom → top.
#[derive(Debug, Clone, PartialEq)]
pub enum PsdItem {
    Layer(PsdLayer),
    /// a folder of layers (bottom → top)
    Group {
        name: String,
        opacity: f64,
        visible: bool,
        children: Vec<PsdLayer>,
    },
}

/// A layer's pixels: straight-alpha RGBA at the canvas's size, and its mask (one byte a
/// pixel, 255 shown), if it has one.
pub type LayerPixels = (Vec<u8>, Option<Vec<u8>>);

fn blend_key(b: &str) -> &'static [u8; 4] {
    match b {
        "normal" => b"norm",
        "multiply" => b"mul ",
        "screen" => b"scrn",
        "overlay" => b"over",
        "darken" => b"dark",
        "lighten" => b"lite",
        "add" => b"lddg",
        _ => b"norm",
    }
}

/// PackBits one row (as Photoshop uses): literal runs and repeats of up to 128 (pairs stay in
/// literal runs, so a row never grows by more than a byte in 128). Appends to `out`.
fn pack_row(src: &[u8], out: &mut Vec<u8>) {
    let len = src.len();
    let mut i = 0;
    while i < len {
        let mut run = 1;
        while i + run < len && run < 128 && src[i + run] == src[i] {
            run += 1;
        }
        if run >= 3 {
            out.push((257 - run) as u8);
            out.push(src[i]);
            i += run;
            continue;
        }
        let mut lit = 1;
        while i + lit < len
            && lit < 128
            && !(i + lit + 2 < len
                && src[i + lit] == src[i + lit + 1]
                && src[i + lit] == src[i + lit + 2])
        {
            lit += 1;
        }
        out.push((lit - 1) as u8);
        out.extend_from_slice(&src[i..i + lit]);
        i += lit;
    }
}

/// One channel's image data with its compression mark: PackBits (1) with the rows' byte
/// counts first, or raw (0) when that's smaller or the channel is empty.
fn channel_block(data: &[u8], cols: usize, rows: usize) -> Vec<u8> {
    let raw = || {
        let mut b = vec![0u8; 2];
        b.extend_from_slice(data);
        b
    };
    if cols == 0 || rows == 0 {
        return raw();
    }
    let mut counts = Vec::with_capacity(rows);
    let mut packed = Vec::new();
    for y in 0..rows {
        let before = packed.len();
        pack_row(&data[y * cols..(y + 1) * cols], &mut packed);
        counts.push((packed.len() - before) as u16);
    }
    if packed.len() + rows * 2 >= data.len() {
        return raw();
    }
    let mut b = Vec::with_capacity(2 + rows * 2 + packed.len());
    b.extend_from_slice(&[0, 1]);
    for c in counts {
        b.extend_from_slice(&c.to_be_bytes());
    }
    b.extend_from_slice(&packed);
    b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Rect {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
}

/// The smallest rectangle around the layer's paint (none: all zero).
fn bounds(rgba: &[u8], w: usize, h: usize) -> Rect {
    let (mut top, mut left, mut bottom, mut right) = (h, w, 0, 0);
    for y in 0..h {
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        let px = row.as_chunks::<4>().0;
        let Some(first) = px.iter().position(|p| p[3] != 0) else {
            continue;
        };
        let last = px.iter().rposition(|p| p[3] != 0).unwrap_or(first);
        top = top.min(y);
        bottom = y + 1;
        left = left.min(first);
        right = right.max(last + 1);
    }
    if bottom == 0 {
        return Rect::default();
    }
    Rect {
        top: top as i32,
        left: left as i32,
        bottom: bottom as i32,
        right: right as i32,
    }
}

/// One channel of `rgba` (0–3) cut to `r`.
fn plane(rgba: &[u8], w: usize, r: Rect, ch: usize) -> Vec<u8> {
    let (rw, rh) = ((r.right - r.left) as usize, (r.bottom - r.top) as usize);
    let mut out = Vec::with_capacity(rw * rh);
    for y in 0..rh {
        let row = ((y + r.top as usize) * w + r.left as usize) * 4;
        out.extend(rgba[row..row + rw * 4].iter().skip(ch).step_by(4));
    }
    out
}

/// A layer's channel blocks (-1 alpha, 0 red, 1 green, 2 blue, then -2 its mask) and its
/// rectangle.
fn layer_blocks(px: &LayerPixels, w: usize, h: usize) -> (Rect, Vec<(i16, Vec<u8>)>) {
    let (rgba, mask) = px;
    let rect = bounds(rgba, w, h);
    let (cw, ch) = (
        (rect.right - rect.left) as usize,
        (rect.bottom - rect.top) as usize,
    );
    let mut v: Vec<(i16, Vec<u8>)> = [-1i16, 0, 1, 2]
        .iter()
        .map(|&id| {
            let c = if id == -1 { 3 } else { id as usize };
            (id, channel_block(&plane(rgba, w, rect, c), cw, ch))
        })
        .collect();
    if let Some(m) = mask {
        v.push((-2, channel_block(m, w, h)));
    }
    (rect, v)
}

/// A layer record's fields.
struct Record<'a> {
    name: &'a str,
    rect: Rect,
    /// channel id, its block's length
    channels: Vec<(i16, u32)>,
    blend: &'static [u8; 4],
    opacity: f64,
    clip: bool,
    visible: bool,
    mask: Option<Rect>,
    /// lsct: 1 an open folder, 3 a folder's end
    section: Option<u32>,
}

fn pascal(o: &mut Vec<u8>, name: &str) {
    // the legacy name: printable ASCII ('?' for each UTF-16 unit that isn't), up to 255
    // bytes, padded to a multiple of 4 with its length byte
    let ascii: Vec<u8> = name
        .encode_utf16()
        .map(|u| {
            if (0x20..=0x7e).contains(&u) {
                u as u8
            } else {
                b'?'
            }
        })
        .take(255)
        .collect();
    o.push(ascii.len() as u8);
    o.extend_from_slice(&ascii);
    let mut n = ascii.len() + 1;
    while !n.is_multiple_of(4) {
        o.push(0);
        n += 1;
    }
}

/// `body`'s bytes with their length first (padded to a multiple of `pad`).
fn sized(o: &mut Vec<u8>, pad: usize, body: impl FnOnce(&mut Vec<u8>)) {
    let mut b = Vec::new();
    body(&mut b);
    while !b.len().is_multiple_of(pad) {
        b.push(0);
    }
    o.extend_from_slice(&(b.len() as u32).to_be_bytes());
    o.extend_from_slice(&b);
}

fn opacity_byte(o: f64) -> u8 {
    (o.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn write_record(o: &mut Vec<u8>, r: &Record) {
    for v in [r.rect.top, r.rect.left, r.rect.bottom, r.rect.right] {
        o.extend_from_slice(&v.to_be_bytes());
    }
    o.extend_from_slice(&(r.channels.len() as u16).to_be_bytes());
    for (id, len) in &r.channels {
        o.extend_from_slice(&(*id as u16).to_be_bytes());
        o.extend_from_slice(&len.to_be_bytes());
    }
    o.extend_from_slice(b"8BIM");
    o.extend_from_slice(r.blend);
    o.push(opacity_byte(r.opacity));
    o.push(r.clip as u8);
    // bit 1: hidden; bits 3 and 4: "pixel data irrelevant" (a folder's markers)
    o.push((if r.visible { 0 } else { 2 }) | if r.section.is_some() { 8 | 16 } else { 0 });
    o.push(0);
    sized(o, 1, |x| {
        match r.mask {
            Some(m) => {
                x.extend_from_slice(&20u32.to_be_bytes());
                for v in [m.top, m.left, m.bottom, m.right] {
                    x.extend_from_slice(&v.to_be_bytes());
                }
                // outside the mask: shown
                x.extend_from_slice(&[255, 0, 0, 0]);
            }
            None => x.extend_from_slice(&0u32.to_be_bytes()),
        }
        // blending ranges
        x.extend_from_slice(&0u32.to_be_bytes());
        pascal(x, r.name);
        x.extend_from_slice(b"8BIMluni");
        sized(x, 2, |u| {
            let name: Vec<u16> = r.name.encode_utf16().take(255).collect();
            u.extend_from_slice(&(name.len() as u32).to_be_bytes());
            for c in name {
                u.extend_from_slice(&c.to_be_bytes());
            }
        });
        if let Some(sec) = r.section {
            x.extend_from_slice(b"8BIMlsct");
            sized(x, 1, |u| {
                u.extend_from_slice(&sec.to_be_bytes());
                if sec == 1 {
                    u.extend_from_slice(b"8BIM");
                    u.extend_from_slice(r.blend);
                }
            });
        }
    });
}

/// The records' order: (a layer's index in `pixels` order, or a folder's marker).
enum Entry<'a> {
    Layer(usize, &'a PsdLayer),
    End,
    Folder {
        name: &'a str,
        opacity: f64,
        visible: bool,
    },
}

fn entries(items: &[PsdItem]) -> Vec<Entry<'_>> {
    let mut v = Vec::new();
    let mut n = 0;
    for it in items {
        match it {
            PsdItem::Layer(l) => {
                v.push(Entry::Layer(n, l));
                n += 1;
            }
            PsdItem::Group {
                name,
                opacity,
                visible,
                children,
            } => {
                // a folder: its end marker, its layers, then the folder itself (bottom → top)
                v.push(Entry::End);
                for c in children {
                    v.push(Entry::Layer(n, c));
                    n += 1;
                }
                v.push(Entry::Folder {
                    name,
                    opacity: *opacity,
                    visible: *visible,
                });
            }
        }
    }
    v
}

const EMPTY_CHANNELS: [(i16, u32); 4] = [(-1, 2), (0, 2), (1, 2), (2, 2)];

/// Writes the file for a `width` × `height` document: `composite` the flattened picture
/// (RGBA, alpha ignored), `items` bottom → top, `pixels` each layer's pixels by its index
/// (layers counted bottom → top through the folders; each is asked for twice).
pub fn write_psd<W: Write>(
    mut w: W,
    width: u32,
    height: u32,
    composite: &[u8],
    items: &[PsdItem],
    pixels: &mut dyn FnMut(usize) -> LayerPixels,
) -> io::Result<()> {
    let (wu, hu) = (width as usize, height as usize);
    if composite.len() < wu * hu * 4 {
        return Err(io::Error::other("the flattened picture is the wrong size"));
    }
    let list = entries(items);
    // first: each layer's rectangle and channel lengths
    // (rectangle, channels' lengths, masked)
    type Sized = (Rect, Vec<(i16, u32)>, bool);
    let mut sized_layers: Vec<Sized> = Vec::new();
    for e in &list {
        if let Entry::Layer(i, _) = e {
            let px = pixels(*i);
            if px.0.len() != wu * hu * 4 || px.1.as_ref().is_some_and(|m| m.len() != wu * hu) {
                return Err(io::Error::other("a layer's pixels are the wrong size"));
            }
            let (rect, blocks) = layer_blocks(&px, wu, hu);
            sized_layers.push((
                rect,
                blocks.iter().map(|(id, b)| (*id, b.len() as u32)).collect(),
                px.1.is_some(),
            ));
        }
    }
    let mut recs = Vec::new();
    let mut blocks_len: u64 = 0;
    let mut k = 0;
    for e in &list {
        let r = match e {
            Entry::Layer(_, l) => {
                let (rect, ch, masked) = &sized_layers[k];
                k += 1;
                blocks_len += ch.iter().map(|c| c.1 as u64).sum::<u64>();
                Record {
                    name: &l.name,
                    rect: *rect,
                    channels: ch.clone(),
                    blend: blend_key(&l.blend),
                    opacity: l.opacity,
                    clip: l.clip,
                    visible: l.visible,
                    mask: masked.then_some(Rect {
                        top: 0,
                        left: 0,
                        bottom: height as i32,
                        right: width as i32,
                    }),
                    section: None,
                }
            }
            Entry::End => {
                blocks_len += 8;
                Record {
                    name: "</Layer group>",
                    rect: Rect::default(),
                    channels: EMPTY_CHANNELS.to_vec(),
                    blend: b"norm",
                    opacity: 1.0,
                    clip: false,
                    visible: true,
                    section: Some(3),
                    mask: None,
                }
            }
            Entry::Folder {
                name,
                opacity,
                visible,
            } => {
                blocks_len += 8;
                Record {
                    name,
                    rect: Rect::default(),
                    channels: EMPTY_CHANNELS.to_vec(),
                    blend: b"pass",
                    opacity: *opacity,
                    clip: false,
                    visible: *visible,
                    section: Some(1),
                    mask: None,
                }
            }
        };
        write_record(&mut recs, &r);
    }
    // the header
    let mut head = Vec::new();
    head.extend_from_slice(b"8BPS");
    head.extend_from_slice(&1u16.to_be_bytes());
    head.extend_from_slice(&[0; 6]);
    head.extend_from_slice(&3u16.to_be_bytes());
    head.extend_from_slice(&height.to_be_bytes());
    head.extend_from_slice(&width.to_be_bytes());
    head.extend_from_slice(&8u16.to_be_bytes());
    head.extend_from_slice(&3u16.to_be_bytes());
    // colour mode data, image resources
    head.extend_from_slice(&[0; 8]);
    // the layers' section: its length, the layer info's (padded to even), the records
    let li = 2 + recs.len() as u64 + blocks_len;
    let li_padded = li + li % 2;
    let lm = 4 + li_padded + 4;
    head.extend_from_slice(&(lm as u32).to_be_bytes());
    head.extend_from_slice(&(li_padded as u32).to_be_bytes());
    head.extend_from_slice(&(list.len() as u16).to_be_bytes());
    w.write_all(&head)?;
    w.write_all(&recs)?;
    drop(recs);
    // the channels' data, a layer at a time
    for e in &list {
        match e {
            Entry::Layer(i, _) => {
                let px = pixels(*i);
                let (_, blocks) = layer_blocks(&px, wu, hu);
                drop(px);
                for (_, b) in blocks {
                    w.write_all(&b)?;
                }
            }
            _ => {
                for _ in 0..4 {
                    w.write_all(&[0, 0])?;
                }
            }
        }
    }
    if li % 2 == 1 {
        w.write_all(&[0])?;
    }
    // the global layer mask: none
    w.write_all(&[0; 4])?;
    // the flattened picture, PackBits: every row's byte count (R, G then B rows), then rows
    let full = Rect {
        top: 0,
        left: 0,
        bottom: height as i32,
        right: width as i32,
    };
    let mut counts = Vec::with_capacity(hu * 3 * 2);
    let mut rows = Vec::new();
    for c in 0..3 {
        let p = plane(composite, wu, full, c);
        for y in 0..hu {
            let before = rows.len();
            pack_row(&p[y * wu..(y + 1) * wu], &mut rows);
            counts.extend_from_slice(&((rows.len() - before) as u16).to_be_bytes());
        }
    }
    w.write_all(&1u16.to_be_bytes())?;
    w.write_all(&counts)?;
    w.write_all(&rows)?;
    w.flush()
}

/// A document in memory, as the current app's `PsdDoc`: for tests and small files.
pub struct PsdDoc {
    pub width: u32,
    pub height: u32,
    pub composite: Vec<u8>,
    pub items: Vec<PsdItem>,
    /// each layer's pixels, in [`write_psd`]'s order
    pub pixels: Vec<LayerPixels>,
}

impl PsdDoc {
    /// The file's bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let px = &self.pixels;
        write_psd(
            &mut out,
            self.width,
            self.height,
            &self.composite,
            &self.items,
            &mut |i| px[i].clone(),
        )
        .expect("writing to memory");
        out
    }
}

#[cfg(test)]
mod tests;
