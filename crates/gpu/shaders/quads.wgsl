// The interface's one shader: every shape, glyph and picture is a quad.
//   kind 0: a rounded rectangle (fill, border, soft shadow), its edges smoothed by distance
//   kind 1: a glyph mask from the atlas (its alpha), in the fill colour
//   kind 2: a colour picture from the atlas (premultiplied), faded by the fill's alpha
//   kind 3: a rounded rectangle filled with a gradient from `fill` to `border_color`,
//           along the direction in uv.xy (1, 0: left to right; 0, 1: top to bottom)
//   kind 4: the canvas, through a map from frame pixels to canvas pixels (uv: a b c d;
//           border_color: tx ty, then the canvas's size), `fill` outside it; params.y = 1
//           shows canvas pixels as squares; params.z = 1 shows the second picture. The
//           pictures are straight RGBA.
// Colours are premultiplied; blending is "over".

struct Push {
    screen: vec2<f32>,
}
var<immediate> pc: Push; // (Vulkan push constants)

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var canvas: texture_2d<f32>;
@group(0) @binding(3) var second: texture_2d<f32>;

struct Quad {
    @location(0) rect: vec4<f32>,         // x, y, w, h (pixels)
    @location(1) fill: vec4<f32>,         // premultiplied
    @location(2) border_color: vec4<f32>, // premultiplied
    @location(3) uv: vec4<f32>,           // atlas u0, v0, u1, v1
    @location(4) params: vec4<f32>,       // radius, border width, shadow blur, kind
    @location(5) clip: vec4<f32>,         // x0, y0, x1, y1 (pixels)
}

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) fill: vec4<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) uv: vec2<f32>,
    @location(5) params: vec4<f32>,
    @location(6) px: vec2<f32>,
    @location(7) clip: vec4<f32>,
    @location(8) uv_dir: vec2<f32>,
    @location(9) map: vec4<f32>,
    @location(10) map_t: vec4<f32>,
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, q: Quad) -> Out {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    // a shadow reaches beyond its rectangle
    let pad = select(0.0, q.params.z * 2.0, q.params.w == 0.0);
    let p = q.rect.xy - vec2<f32>(pad) + corner * (q.rect.zw + vec2<f32>(2.0 * pad));
    var o: Out;
    // (WGSL's clip space has y up, as WebGPU's; naga turns it into Vulkan's)
    o.pos = vec4<f32>(p.x / pc.screen.x * 2.0 - 1.0, 1.0 - p.y / pc.screen.y * 2.0, 0.0, 1.0);
    o.local = p - q.rect.xy - q.rect.zw * 0.5;
    o.half_size = q.rect.zw * 0.5;
    o.fill = q.fill;
    o.border_color = q.border_color;
    o.uv = mix(q.uv.xy, q.uv.zw, corner);
    o.params = q.params;
    o.px = p;
    o.clip = q.clip;
    o.uv_dir = q.uv.xy;
    o.map = q.uv;
    o.map_t = q.border_color;
    return o;
}

// distance to a rounded box (negative inside)
fn round_box(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    if (i.px.x < i.clip.x || i.px.y < i.clip.y || i.px.x >= i.clip.z || i.px.y >= i.clip.w) {
        discard;
    }
    let kind = i.params.w;
    if (kind == 4.0) {
        let size = i.map_t.zw;
        let c = vec2<f32>(
            i.map.x * i.px.x + i.map.y * i.px.y + i.map_t.x,
            i.map.z * i.px.x + i.map.w * i.px.y + i.map_t.y,
        );
        // its rounded corners (params.x)
        let cover = clamp(0.5 - round_box(i.local, i.half_size, min(i.params.x, min(i.half_size.x, i.half_size.y))), 0.0, 1.0);
        if (c.x < 0.0 || c.y < 0.0 || c.x >= size.x || c.y >= size.y) {
            return i.fill * cover;
        }
        var t: vec4<f32>;
        if (i.params.z == 1.0) {
            if (i.params.y == 1.0) {
                t = textureLoad(second, vec2<i32>(floor(c)), 0);
            } else {
                t = textureSampleLevel(second, samp, c / size, 0.0);
            }
        } else if (i.params.y == 1.0) {
            t = textureLoad(canvas, vec2<i32>(floor(c)), 0);
        } else {
            t = textureSampleLevel(canvas, samp, c / size, 0.0);
        }
        return vec4<f32>(t.rgb * t.a, t.a) * cover;
    }
    if (kind == 1.0) {
        return i.fill * textureSampleLevel(atlas, samp, i.uv, 0.0).a;
    }
    if (kind == 2.0) {
        return textureSampleLevel(atlas, samp, i.uv, 0.0) * i.fill.a;
    }
    let r = min(i.params.x, min(i.half_size.x, i.half_size.y));
    let d = round_box(i.local, i.half_size, r);
    if (kind == 3.0) {
        // where along the direction this pixel is, 0 to 1 across the rectangle
        let dir = i.uv_dir;
        let span = abs(dir.x) * i.half_size.x + abs(dir.y) * i.half_size.y;
        let t = clamp((dot(i.local, dir) / max(span, 0.0001)) * 0.5 + 0.5, 0.0, 1.0);
        return mix(i.fill, i.border_color, t) * clamp(0.5 - d, 0.0, 1.0);
    }
    let blur = i.params.z;
    if (blur > 0.0) {
        // a soft shadow: fades over `blur` pixels either side of the edge
        let a = 1.0 - smoothstep(-blur, blur, d);
        return i.fill * a;
    }
    let cover = clamp(0.5 - d, 0.0, 1.0);
    let bw = i.params.y;
    if (bw > 0.0) {
        let inner = clamp(0.5 - (d + bw), 0.0, 1.0);
        return mix(i.border_color, i.fill, inner) * cover;
    }
    return i.fill * cover;
}
