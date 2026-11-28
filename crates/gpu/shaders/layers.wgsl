// The layers composited on the GPU (`Compositor`, src/layers.rs): each pixel of the tiles
// that changed, from every layer's tile there, over white paper, as the brush engine's
// `composite_pixel_on` does it (crates/brush/src/layers.rs), then the onion skin multiplied
// underneath as the app's `onion::compose` does. The arithmetic is the CPU's, step by step,
// so the two agree to within one level a channel.
//
// Tiles are 64 x 64 premultiplied linear fix15 RGBA (0..=32768), two words a pixel.

struct Frame {
    words: array<u32>,
}

@group(0) @binding(0) var<storage, read> atlas: array<u32>;
@group(0) @binding(1) var<storage, read> frame: Frame;
// linear fix15 (0..=32768) -> sRGB 0-1: the brush engine's own table
@group(0) @binding(2) var<storage, read> lut: array<f32>;
// the picture, BGRA8 a word
@group(0) @binding(3) var<storage, read_write> out: array<u32>;
// the bottom layers kept composited (the prefix): each pixel's colour and the alpha clipped
// layers show through after them, four words a pixel
@group(0) @binding(4) var<storage, read_write> prefix: array<u32>;

const EMPTY: u32 = 0xffffffffu;
const ONE: u32 = 32768u;
// the frame's header (see `Compositor::write_frame`)
const H_W: u32 = 0u;
const H_H: u32 = 1u;
const H_TX: u32 = 2u;
const H_NT: u32 = 3u;
const H_NL: u32 = 4u;
const H_NO: u32 = 5u;
const H_PK: u32 = 7u;
const REBUILD: u32 = 0x80000000u;
const LAYERS: u32 = 16u;
const ONIONS: u32 = 144u;
const DIRTY: u32 = 160u;

fn w(i: u32) -> u32 {
    return frame.words[i];
}

fn px(slot: u32, i: u32) -> vec4<u32> {
    let b = slot * 8192u + i * 2u;
    let lo = atlas[b];
    let hi = atlas[b + 1u];
    return vec4<u32>(lo & 0xffffu, lo >> 16u, hi & 0xffffu, hi >> 16u);
}

// lut[min(c * 32768 / a, 32768)], the integer division done in floats and made exact
fn ch(c: u32, a: u32) -> f32 {
    if a == ONE {
        return lut[min(c, ONE)];
    }
    let n = c * ONE;
    var q = min(u32(f32(n) / f32(a)), ONE + 1u);
    if q * a > n {
        q -= 1u;
    } else if (q + 1u) * a <= n {
        q += 1u;
    }
    return lut[min(q, ONE)];
}

// (unpremultiplied sRGB colour, alpha 0-1)
fn unpremul(p: vec4<u32>) -> vec4<f32> {
    if p.w == 0u {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(ch(p.x, p.w), ch(p.y, p.w), ch(p.z, p.w), f32(p.w) / f32(ONE));
}

fn mask_value(p: vec4<u32>) -> f32 {
    let u = unpremul(p);
    let lum = 0.2126 * u.x + 0.7152 * u.y + 0.0722 * u.z;
    return clamp(1.0 - u.w + u.w * lum, 0.0, 1.0);
}

fn mix_one(m: u32, cb: f32, cs: f32) -> f32 {
    switch m {
        case 1u: {
            return cb * cs;
        }
        case 2u: {
            return cb + cs - cb * cs;
        }
        case 3u: {
            if cb <= 0.5 {
                return 2.0 * cb * cs;
            }
            return 1.0 - 2.0 * (1.0 - cb) * (1.0 - cs);
        }
        case 4u: {
            return min(cb, cs);
        }
        case 5u: {
            return max(cb, cs);
        }
        case 6u: {
            return min(cb + cs, 1.0);
        }
        default: {
            return cs;
        }
    }
}

// round(v * 255) for v in 0..=1, as Rust's `round` (half away from zero)
fn to8(v: f32) -> u32 {
    return u32(floor(v * 255.0 + 0.5));
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let entry = w(DIRTY + g.z);
    let t = entry & ~REBUILD;
    let rebuild = (entry & REBUILD) != 0u;
    let tx = w(H_TX);
    let nt = w(H_NT);
    let x = (t % tx) * 64u + g.x;
    let y = (t / tx) * 64u + g.y;
    let width = w(H_W);
    if x >= width || y >= w(H_H) {
        return;
    }
    let i = g.y * 64u + g.x;
    let nl = w(H_NL);
    let no = w(H_NO);
    let colour_table = DIRTY + nt;
    let mask_table = colour_table + 32u * nt;
    let onion_table = mask_table + 32u * nt;

    var dst = vec3<f32>(1.0, 1.0, 1.0);
    // the alpha of the nearest unclipped layer: what clipped layers above it show through
    var base = 0.0;
    // the prefix: gone on from (kept), or made again on the way
    let pk = w(H_PK);
    let q = (y * width + x) * 4u;
    var first = 0u;
    if pk > 0u && !rebuild {
        dst = vec3<f32>(bitcast<f32>(prefix[q]), bitcast<f32>(prefix[q + 1u]), bitcast<f32>(prefix[q + 2u]));
        base = bitcast<f32>(prefix[q + 3u]);
        first = pk;
    }
    for (var l = first; l < nl; l++) {
        if rebuild && l == pk {
            prefix[q] = bitcast<u32>(dst.x);
            prefix[q + 1u] = bitcast<u32>(dst.y);
            prefix[q + 2u] = bitcast<u32>(dst.z);
            prefix[q + 3u] = bitcast<u32>(base);
        }
        let opacity = bitcast<f32>(w(LAYERS + l * 4u));
        let blend = w(LAYERS + l * 4u + 1u);
        let flags = w(LAYERS + l * 4u + 2u);
        let visible = (flags & 1u) != 0u;
        let clip = (flags & 2u) != 0u;
        let slot = w(colour_table + l * nt + t);
        if slot == EMPTY {
            if !clip {
                base = 0.0;
            }
            continue;
        }
        let u = unpremul(px(slot, i));
        let ms = w(mask_table + l * nt + t);
        var m = 1.0;
        if ms != EMPTY {
            m = mask_value(px(ms, i));
        }
        var a = u.w * opacity * m;
        if clip {
            a *= base;
        } else {
            base = select(0.0, a, visible);
        }
        if !visible || a <= 0.0 {
            continue;
        }
        for (var c = 0u; c < 3u; c++) {
            let b = mix_one(blend, dst[c], u[c]);
            dst[c] += (b - dst[c]) * a;
        }
    }
    if rebuild && pk == nl {
        prefix[q] = bitcast<u32>(dst.x);
        prefix[q + 1u] = bitcast<u32>(dst.y);
        prefix[q + 2u] = bitcast<u32>(dst.z);
        prefix[q + 3u] = bitcast<u32>(base);
    }
    var p = vec3<u32>(to8(clamp(dst.x, 0.0, 1.0)), to8(clamp(dst.y, 0.0, 1.0)), to8(clamp(dst.z, 0.0, 1.0)));

    // the onion skin: tinted lines laid over each other, farthest first
    if no > 0u {
        var rgb = vec3<f32>(0.0);
        var alpha = 0.0;
        for (var e = 0u; e < no; e++) {
            let slot = w(onion_table + e * nt + t);
            if slot == EMPTY {
                continue;
            }
            let q = px(slot, i);
            let sa = to8(f32(q.w) / f32(ONE));
            if sa == 0u {
                continue;
            }
            let tint = vec3<f32>(
                bitcast<f32>(w(ONIONS + e * 4u)),
                bitcast<f32>(w(ONIONS + e * 4u + 1u)),
                bitcast<f32>(w(ONIONS + e * 4u + 2u)),
            );
            let op = bitcast<f32>(w(ONIONS + e * 4u + 3u));
            let a = f32(sa) / 255.0 * op;
            let keep = alpha * (1.0 - a);
            let o = a + keep;
            rgb = (tint * a + rgb * keep) / max(o, 1e-6);
            alpha = o;
        }
        if alpha > 0.0 {
            let k = to8(alpha);
            if k != 0u {
                let n = vec3<u32>(to8(rgb.x), to8(rgb.y), to8(rgb.z));
                // p x (1 - k + k * tint), in 0-255 integers
                let m = vec3<u32>(65025u) - k * (vec3<u32>(255u) - n);
                p = (p * m + vec3<u32>(32512u)) / 65025u;
            }
        }
    }
    out[y * width + x] = p.z | (p.y << 8u) | (p.x << 16u) | (255u << 24u);
}
