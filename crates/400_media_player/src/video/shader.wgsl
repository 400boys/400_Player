// 400_media_player video shader.
//
// One uniform block drives three entry-point pairs:
//   vs_quad / fs_video        the picture (YUV or RGBA planes -> display RGB, with Catmull-Rom 5-tap luma sharpening)
//   vs_quad / fs_ambient_src  the same picture rendered to a tiny texture (ambient colour)
//   vs_quad / fs_ambient_draw a soft glow built from that tiny texture, painted behind the picture
//
// Output is gamma-encoded ("sRGB-ish") RGB in a non-sRGB framebuffer, which is what egui expects.

struct Params {
    dst: vec4<f32>,     // quad in NDC: left, bottom, right, top
    uvm: vec4<f32>,     // 2x2 display->source matrix (row major), applied around the centre
    row0: vec4<f32>,    // YUV -> R'G'B' matrix rows
    row1: vec4<f32>,
    row2: vec4<f32>,
    gam0: vec4<f32>,    // linear BT.2020 -> BT.709 rows
    gam1: vec4<f32>,
    gam2: vec4<f32>,
    range: vec4<f32>,   // y_offset, y_scale, chroma_offset, chroma_scale
    misc: vec4<f32>,    // layout (0 planar, 1 semi-planar, 2 rgba), transfer (0 sdr, 1 pq, 2 hlg), hdr peak (x SDR white), bit scale
    adjust: vec4<f32>,  // brightness, contrast, saturation, gamma
    flags: vec4<f32>,   // wide gamut sdr (0/1), dither amplitude, opacity, ambient strength
    tgt: vec4<f32>,     // x: framebuffer is sRGB (needs linear output), y/z: ambient texture size in texels
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var t0: texture_2d<f32>;
@group(0) @binding(3) var t1: texture_2d<f32>;
@group(0) @binding(4) var t2: texture_2d<f32>;
@group(0) @binding(5) var tamb: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_quad(@builtin(vertex_index) vi: u32) -> VsOut {
    // Triangle strip: (0,0) (1,0) (0,1) (1,1) with (0,0) at the top-left.
    let cx = f32(vi & 1u);
    let cy = f32((vi >> 1u) & 1u);
    var o: VsOut;
    o.pos = vec4<f32>(mix(p.dst.x, p.dst.z, cx), mix(p.dst.w, p.dst.y, cy), 0.0, 1.0);
    o.uv = vec2<f32>(cx, cy);
    return o;
}

fn mul3(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(a.xyz, v), dot(b.xyz, v), dot(c.xyz, v));
}

// ---- transfer functions -------------------------------------------------------------------------

// SMPTE ST 2084. Input: non-linear signal 0..1. Output: display light, 1.0 = 10 000 nits.
fn pq_eotf(e: vec3<f32>) -> vec3<f32> {
    let m1 = 0.1593017578125;
    let m2 = 78.84375;
    let c1 = 0.8359375;
    let c2 = 18.8515625;
    let c3 = 18.6875;
    let ep = pow(max(e, vec3<f32>(0.0)), vec3<f32>(1.0 / m2));
    let num = max(ep - vec3<f32>(c1), vec3<f32>(0.0));
    let den = vec3<f32>(c2) - c3 * ep;
    return pow(num / den, vec3<f32>(1.0 / m1));
}

// ARIB STD-B67 inverse OETF: signal 0..1 -> normalised scene light 0..1.
fn hlg_inverse_oetf(e: vec3<f32>) -> vec3<f32> {
    let a = 0.17883277;
    let b = 0.28466892;
    let c = 0.55991073;
    let low = e * e / 3.0;
    let high = (exp((e - vec3<f32>(c)) / a) + vec3<f32>(b)) / 12.0;
    return select(high, low, e <= vec3<f32>(0.5));
}

fn srgb_oetf(l: vec3<f32>) -> vec3<f32> {
    let c = clamp(l, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn srgb_eotf(e: vec3<f32>) -> vec3<f32> {
    let c = clamp(e, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// Highlight roll-off: linear up to the knee, then an asymptotic curve that reaches 1.0 at `w`
// (the peak of the content, in units of SDR white).
fn tm_curve(l: f32, w: f32) -> f32 {
    let ks = 0.6;
    if (l <= ks) {
        return l;
    }
    let h = 1.0 - ks;
    let x = l - ks;
    let xw = max(w - ks, 0.01);
    return ks + h * (x / (x + h)) / (xw / (xw + h));
}

// Hue-preserving tone mapping. The roll-off is driven by a norm that is the luminance for greys
// and the largest channel for saturated colours, so a bright red stays red instead of washing out;
// nothing below the knee is touched and nothing ends above 1.0.
fn tonemap(lin: vec3<f32>, peak: f32) -> vec3<f32> {
    let luma = dot(lin, vec3<f32>(0.2627, 0.6780, 0.0593));
    let mx = max(lin.r, max(lin.g, lin.b));
    let mn = min(lin.r, min(lin.g, lin.b));
    var sat = 0.0;
    if (mx > 1e-6) {
        sat = (mx - mn) / mx;
    }
    let n = luma + sat * (mx - luma);
    var scale = 1.0;
    if (n > 1e-6) {
        scale = tm_curve(n, peak) / n;
    }
    if (mx * scale > 1.0) {
        scale = 1.0 / mx;
    }
    return lin * scale;
}

// ---- the picture --------------------------------------------------------------------------------

// Fast 5-tap Catmull-Rom bicubic luma reconstruction with anti-ringing clamp.
fn sample_luma_bicubic(uv: vec2<f32>) -> f32 {
    let dims = vec2<f32>(textureDimensions(t0, 0));
    if (dims.x <= 4.0 || dims.y <= 4.0) {
        return textureSampleLevel(t0, samp, uv, 0.0).r;
    }
    let inv = 1.0 / dims;
    let pos = uv * dims - vec2<f32>(0.5);
    let f = fract(pos);
    let i = floor(pos);

    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);

    let w12 = w1 + w2;
    let off12 = w2 / max(w12, vec2<f32>(1e-5));

    let tc0 = (i - vec2<f32>(0.5)) * inv;
    let tc3 = (i + vec2<f32>(2.5)) * inv;
    let tc12 = (i + vec2<f32>(0.5) + off12) * inv;

    let s_c = textureSampleLevel(t0, samp, tc12, 0.0).r;
    let s_l = textureSampleLevel(t0, samp, vec2<f32>(tc0.x, tc12.y), 0.0).r;
    let s_r = textureSampleLevel(t0, samp, vec2<f32>(tc3.x, tc12.y), 0.0).r;
    let s_t = textureSampleLevel(t0, samp, vec2<f32>(tc12.x, tc0.y), 0.0).r;
    let s_b = textureSampleLevel(t0, samp, vec2<f32>(tc12.x, tc3.y), 0.0).r;

    let wl = w0.x * w12.y;
    let wr = w3.x * w12.y;
    let wt = w12.x * w0.y;
    let wb = w12.x * w3.y;
    let wc = w12.x * w12.y;
    let wsum = wl + wr + wt + wb + wc;
    let raw = (s_l * wl + s_r * wr + s_t * wt + s_b * wb + s_c * wc) / max(wsum, 1e-5);
    let lo = min(s_c, min(min(s_l, s_r), min(s_t, s_b)));
    let hi = max(s_c, max(max(s_l, s_r), max(s_t, s_b)));
    return clamp(raw, lo, hi);
}

fn sample_encoded(uv: vec2<f32>) -> vec3<f32> {
    let kind = u32(p.misc.x + 0.5);
    if (kind == 2u) {
        return textureSampleLevel(t0, samp, uv, 0.0).rgb;
    }
    let bs = p.misc.w;
    let y = sample_luma_bicubic(uv) * bs;
    var cb: f32;
    var cr: f32;
    if (kind == 0u) {
        cb = textureSampleLevel(t1, samp, uv, 0.0).r * bs;
        cr = textureSampleLevel(t2, samp, uv, 0.0).r * bs;
    } else {
        let c = textureSampleLevel(t1, samp, uv, 0.0).rg * bs;
        cb = c.x;
        cr = c.y;
    }
    let yy = (y - p.range.x) * p.range.y;
    let u = (cb - p.range.z) * p.range.w;
    let v = (cr - p.range.z) * p.range.w;
    return mul3(p.row0, p.row1, p.row2, vec3<f32>(yy, u, v));
}

fn shade(uv: vec2<f32>) -> vec3<f32> {
    var rgb = sample_encoded(uv);
    let transfer = u32(p.misc.y + 0.5);
    if (transfer == 1u) {
        // HDR10 (PQ) -> SDR
        var lin = pq_eotf(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0))) * (10000.0 / 203.0);
        lin = tonemap(lin, p.misc.z);
        lin = clamp(mul3(p.gam0, p.gam1, p.gam2, lin), vec3<f32>(0.0), vec3<f32>(1.0));
        rgb = srgb_oetf(lin);
    } else if (transfer == 2u) {
        // HLG -> SDR (BT.2100 OOTF for a 1000-nit display, expressed in units of 203-nit white)
        let scene = hlg_inverse_oetf(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
        let ys = dot(scene, vec3<f32>(0.2627, 0.6780, 0.0593));
        var lin = scene * (pow(max(ys, 1e-6), 0.2) * (1000.0 / 203.0));
        lin = tonemap(lin, p.misc.z);
        lin = clamp(mul3(p.gam0, p.gam1, p.gam2, lin), vec3<f32>(0.0), vec3<f32>(1.0));
        rgb = srgb_oetf(lin);
    } else if (p.flags.x > 0.5) {
        // SDR content with wide-gamut primaries
        let lin = pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(2.4));
        let l709 = clamp(mul3(p.gam0, p.gam1, p.gam2, lin), vec3<f32>(0.0), vec3<f32>(1.0));
        rgb = pow(l709, vec3<f32>(1.0 / 2.4));
    }
    // User adjustments (identity unless changed): brightness, contrast, saturation, gamma.
    if (p.adjust.x != 0.0 || p.adjust.y != 1.0 || p.adjust.z != 1.0 || p.adjust.w != 1.0) {
        rgb = (rgb - vec3<f32>(0.5)) * p.adjust.y + vec3<f32>(0.5) + vec3<f32>(p.adjust.x);
        let l = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        rgb = mix(vec3<f32>(l), rgb, p.adjust.z);
        rgb = pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / p.adjust.w));
    }
    return rgb;
}

// Interleaved gradient noise (Jimenez, 2014): a few multiplies, no transcendental function, and the
// pattern has no visible structure at the amplitude used (about one 8-bit level).
fn noise(v: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(v, vec2<f32>(0.06711056, 0.00583715))));
}

fn to_target(rgb: vec3<f32>) -> vec3<f32> {
    if (p.tgt.x > 0.5) {
        return srgb_eotf(rgb);
    }
    return rgb;
}

fn display_to_source(uv: vec2<f32>) -> vec2<f32> {
    let c = uv - vec2<f32>(0.5);
    return vec2<f32>(p.uvm.x * c.x + p.uvm.y * c.y, p.uvm.z * c.x + p.uvm.w * c.y) + vec2<f32>(0.5);
}

@fragment
fn fs_video(in: VsOut) -> @location(0) vec4<f32> {
    var rgb = shade(display_to_source(in.uv));
    // Break up banding of high bit depth sources on an 8-bit surface (8-bit sources skip it).
    if (p.flags.y > 0.0) {
        rgb = rgb + vec3<f32>((noise(in.pos.xy) - 0.5) * p.flags.y);
    }
    rgb = to_target(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    let a = p.flags.z;
    return vec4<f32>(rgb * a, a);
}

// ---- ambient glow -------------------------------------------------------------------------------

// Renders the (rotated) picture into the tiny ambient texture, averaging a 4x4 block of taps so
// the result does not flicker on detailed content.
@fragment
fn fs_ambient_src(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(p.tgt.y, p.tgt.z);
    let texel = 1.0 / dims;
    var acc = vec3<f32>(0.0);
    for (var j = 0; j < 4; j = j + 1) {
        for (var i = 0; i < 4; i = i + 1) {
            let off = (vec2<f32>(f32(i), f32(j)) - vec2<f32>(1.5)) * 0.25 * texel;
            acc = acc + shade(display_to_source(in.uv + off));
        }
    }
    return vec4<f32>(clamp(acc / 16.0, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// Soft glow: a 3x3 tent blur of the tiny texture, a little more saturated, faded by `flags.w`.
@fragment
fn fs_ambient_draw(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(p.tgt.y, p.tgt.z);
    let texel = 1.0 / dims;
    var acc = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var j = -1; j <= 1; j = j + 1) {
        for (var i = -1; i <= 1; i = i + 1) {
            let w = (2.0 - abs(f32(i))) * (2.0 - abs(f32(j)));
            acc = acc + textureSampleLevel(tamb, samp, in.uv + vec2<f32>(f32(i), f32(j)) * texel * 1.5, 0.0).rgb * w;
            wsum = wsum + w;
        }
    }
    var c = acc / wsum;
    let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = clamp(mix(vec3<f32>(l), c, 1.35), vec3<f32>(0.0), vec3<f32>(1.0));
    let a = p.flags.w;
    return vec4<f32>(to_target(c) * a, a);
}
