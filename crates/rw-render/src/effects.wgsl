// Effect, transition and merge passes (`common.wgsl` is prepended).
//
// They work on one clip drawn on its own: a full-canvas image in linear
// light with premultiplied alpha. Effects change it, transitions combine two
// of them, and the merge pass blends the result onto the canvas.

struct Fx {
    // Output size in pixels, kind, intensity.
    a: vec4<f32>,
    // Seconds into the clip, share of the clip played (or transition
    // progress), unused, sub-pass (or blend mode for the merge).
    b: vec4<f32>,
    // Effect parameters.
    p0: vec4<f32>,
    p1: vec4<f32>,
    // The clip's picture: centre and half size in pixels.
    geo: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var lin_sampler: sampler;
@group(0) @binding(2) var<uniform> F: Fx;
@group(0) @binding(3) var other_tex: texture_2d<f32>;

const TAU: f32 = 6.28318530718;

// Samples outside the frame are transparent.
fn sa(uv: vec2<f32>) -> vec4<f32> {
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return vec4(0.0);
    }
    return textureSampleLevel(src_tex, lin_sampler, uv, 0.0);
}

fn sb(uv: vec2<f32>) -> vec4<f32> {
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return vec4(0.0);
    }
    return textureSampleLevel(other_tex, lin_sampler, uv, 0.0);
}

// Integer hash: identical on every GPU, unlike sin-based noise.
fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rand3(x: u32, y: u32, z: u32) -> f32 {
    return f32(pcg(x ^ pcg(y ^ pcg(z))) >> 8u) / 16777215.0;
}

// The frame number used to vary noise over time (24 changes a second).
fn tick() -> u32 {
    return u32(max(F.b.x, 0.0) * 24.0);
}

// Straight sRGB colour of a premultiplied linear sample, and back.
fn unpremul(c: vec4<f32>) -> vec3<f32> {
    return clamp(to_srgb(c.rgb / max(c.a, 1e-5)), vec3(0.0), vec3(1.0));
}

fn premul(s: vec3<f32>, a: f32) -> vec4<f32> {
    return vec4(to_linear(clamp(s, vec3(0.0), vec3(1.0))) * a, a);
}

// Gaussian blur along `dir` (pixels per tap) with 15 taps.
fn blur_a(uv: vec2<f32>, dir: vec2<f32>) -> vec4<f32> {
    var sum = vec4(0.0);
    var total = 0.0;
    for (var i = -7; i <= 7; i++) {
        let w = exp(-f32(i * i) / 18.0);
        sum += sa(uv + dir * f32(i)) * w;
        total += w;
    }
    return sum / total;
}

// Soft blur of either image for the blur dissolve (golden-angle spiral).
fn spiral(uv: vec2<f32>, radius: f32, from_other: bool) -> vec4<f32> {
    if radius < 0.5 {
        if from_other { return sb(uv); }
        return sa(uv);
    }
    let texel = 1.0 / F.a.xy;
    var sum = vec4(0.0);
    for (var i = 0; i < 24; i++) {
        let r = sqrt((f32(i) + 0.5) / 24.0) * radius;
        let t = f32(i) * 2.39996323;
        let o = vec2(cos(t), sin(t)) * r * texel;
        if from_other { sum += sb(uv + o); } else { sum += sa(uv + o); }
    }
    return sum / 24.0;
}

fn rotate(v: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2(c * v.x - s * v.y, s * v.x + c * v.y);
}

@fragment
fn fs_effect(in: VsOut) -> @location(0) vec4<f32> {
    let size = F.a.xy;
    let texel = 1.0 / size;
    let kind = u32(F.a.z + 0.5);
    let k = clamp(F.a.w, 0.0, 1.0);
    let pass_index = u32(F.b.w + 0.5);
    let uv = in.uv;
    let px = uv * size;
    let c = sa(uv);
    switch kind {
        // Blur: horizontal then vertical.
        case 0u: {
            let stride = k * 0.03 * size.y / 7.0;
            if pass_index == 0u {
                return blur_a(uv, vec2(stride * texel.x, 0.0));
            }
            return blur_a(uv, vec2(0.0, stride * texel.y));
        }
        // Blurred backdrop: an enlarged copy filling the frame, blurred,
        // under the clip.
        case 1u: {
            let stride = (0.02 + 0.06 * k) * size.y / 7.0;
            if pass_index == 0u {
                let half = max(F.geo.zw, vec2(1.0));
                let zoom = max(size.x / (2.0 * half.x), size.y / (2.0 * half.y)) * 1.05;
                let centre = F.geo.xy * texel;
                let p = centre + (uv - centre) / zoom;
                // Stay inside the picture so the edges don't go transparent.
                let lo = (F.geo.xy - half + 1.0) * texel;
                let hi = (F.geo.xy + half - 1.0) * texel;
                return sa(clamp(p, lo, hi));
            }
            if pass_index == 1u {
                return blur_a(uv, vec2(stride * texel.x, 0.0));
            }
            let back = blur_a(uv, vec2(0.0, stride * texel.y));
            let front = sb(uv);
            return front + back * (1.0 - front.a);
        }
        // Glitch: some bands slide sideways, colours split.
        case 2u: {
            let band = u32(floor(uv.y * 24.0));
            var p = uv;
            if rand3(band, tick(), 7u) < 0.35 * k {
                p.x += (rand3(band, tick(), 11u) - 0.5) * 0.15 * k;
            }
            let off = vec2(0.012 * k * rand3(tick(), 3u, 5u), 0.0);
            let r = sa(p + off);
            let g = sa(p);
            let b = sa(p - off);
            return vec4(r.r, g.g, b.b, max(max(r.a, g.a), b.a));
        }
        // Colour split.
        case 3u: {
            let off = vec2(0.012 * k, 0.0);
            let r = sa(uv + off);
            let b = sa(uv - off);
            return vec4(r.r, c.g, b.b, max(max(r.a, c.a), b.a));
        }
        // Tape: line jitter, colour bleed, scan lines, noise, faded colour.
        case 4u: {
            let row = u32(floor(px.y / 2.0));
            let jitter = (rand3(row, tick(), 13u) - 0.5) * 0.004 * k;
            let p = uv + vec2(jitter, 0.0);
            let off = vec2(0.005 * k, 0.0);
            let base = sa(p);
            var s = unpremul(vec4(sa(p + off).r, base.g, sa(p - off).b, base.a));
            s = mix(s, vec3(luma(s)), 0.3 * k);
            s = s * (1.0 - 0.18 * k * f32(row % 2u));
            s += (rand3(u32(px.x), u32(px.y), tick()) - 0.5) * 0.12 * k;
            s = s * (1.0 - 0.1 * k) + 0.06 * k;
            return premul(s, base.a);
        }
        // Grain.
        case 5u: {
            let n = rand3(u32(px.x), u32(px.y), tick() + 101u) - 0.5;
            let s = unpremul(c) + n * 0.2 * k;
            return premul(s, c.a);
        }
        // Vignette.
        case 6u: {
            let aspect = size.x / size.y;
            let d = length((uv - 0.5) * vec2(aspect, 1.0)) / length(vec2(aspect, 1.0) * 0.5);
            let inner = mix(0.05, 0.8, F.p0.x);
            let f = 1.0 - k * smoothstep(inner, 1.15, d);
            return vec4(c.rgb * f * f, c.a);
        }
        // Pixelate.
        case 7u: {
            let bs = 2.0 + k * 0.06 * size.y;
            let cell = (floor(px / bs) + 0.5) * bs;
            return sa(cell * texel);
        }
        // Slow zoom: scale over the clip, drifting towards the pan point.
        case 8u: {
            let t = clamp(F.b.y, 0.0, 1.0);
            let amount = select(1.0 - t, t, F.p0.z > 0.5);
            let s = 1.0 + 0.3 * k * amount;
            let centre = 0.5 + F.p0.xy * 0.5 * (1.0 - 1.0 / s);
            return sa(centre + (uv - 0.5) / s);
        }
        // Shake: jumps a little every frame, zoomed so edges stay hidden.
        case 9u: {
            let f = tick();
            let off = (vec2(rand3(f, 1u, 17u), rand3(f, 2u, 17u)) - 0.5) * 0.03 * k;
            let zoom = 1.0 + 0.06 * k;
            return sa(0.5 + (uv - 0.5) / zoom + off);
        }
        // Haze: lifted, warm, low contrast.
        case 10u: {
            var s = unpremul(c);
            s = s + (1.0 - s) * 0.35 * k * vec3(1.0, 0.96, 0.9);
            return premul(s, c.a);
        }
        // Sharpen.
        case 11u: {
            let n = sa(uv + vec2(texel.x, 0.0)) + sa(uv - vec2(texel.x, 0.0))
                + sa(uv + vec2(0.0, texel.y)) + sa(uv - vec2(0.0, texel.y));
            let r = c + 1.2 * k * (4.0 * c - n);
            return vec4(clamp(r.rgb, vec3(0.0), vec3(r.a)), clamp(r.a, 0.0, 1.0));
        }
        // Black and white.
        case 12u: {
            let s = unpremul(c);
            return premul(mix(s, vec3(luma(s)), k), c.a);
        }
        // Sepia.
        case 13u: {
            let s = unpremul(c);
            let sep = vec3(
                dot(s, vec3(0.393, 0.769, 0.189)),
                dot(s, vec3(0.349, 0.686, 0.168)),
                dot(s, vec3(0.272, 0.534, 0.131)),
            );
            return premul(mix(s, sep, k), c.a);
        }
        // Invert.
        case 14u: {
            let s = unpremul(c);
            return premul(mix(s, 1.0 - s, k), c.a);
        }
        // Green screen: transparent near the key colour (by chroma), with
        // the key's tint removed from what's left.
        case 15u: {
            let s = unpremul(c);
            let to_cbcr = mat3x2<f32>(
                vec2(-0.1146, 0.5),
                vec2(-0.3854, -0.4542),
                vec2(0.5, -0.0458),
            );
            let key = to_cbcr * F.p0.xyz;
            let chroma = to_cbcr * s;
            let tol = 0.02 + 0.3 * k;
            let soft = 0.02 + 0.3 * F.p0.w;
            let keep = smoothstep(tol, tol + soft, distance(chroma, key));
            // Spill: take the key's direction out of the chroma.
            let dir = key / max(length(key), 1e-5);
            let along = max(dot(chroma, dir), 0.0) * F.p1.x;
            let y = luma(s);
            let fixed = chroma - dir * along;
            let rgb = vec3(
                y + 1.5748 * fixed.y,
                y - 0.1873 * fixed.x - 0.4681 * fixed.y,
                y + 1.8556 * fixed.x,
            );
            return premul(rgb, c.a * keep);
        }
        default: {
            return c;
        }
    }
}

const BLACK: vec4<f32> = vec4(0.0, 0.0, 0.0, 1.0);
const WHITE: vec4<f32> = vec4(1.0, 1.0, 1.0, 1.0);

// Mask that is 1 where `x < edge`, with a soft edge; `edge` runs past both
// ends so the start and end are clean.
fn reveal(x: f32, p: f32) -> f32 {
    let f = 0.02;
    let edge = mix(-f, 1.0 + f, p);
    return 1.0 - smoothstep(edge - f, edge + f, x);
}

// `b` over `a`.
fn over(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return b + a * (1.0 - b.a);
}

// `src_tex` is the outgoing clip, `other_tex` the incoming one.
@fragment
fn fs_transition(in: VsOut) -> @location(0) vec4<f32> {
    let size = F.a.xy;
    let aspect = size.x / size.y;
    let kind = u32(F.a.z + 0.5);
    let p = clamp(F.b.y, 0.0, 1.0);
    let uv = in.uv;
    let a = sa(uv);
    let b = sb(uv);
    switch kind {
        // Cross dissolve.
        case 0u: { return mix(a, b, p); }
        // Through black / white.
        case 1u, 2u: {
            let mid = select(BLACK, WHITE, kind == 2u);
            if p < 0.5 {
                return mix(over(mid, a), mid, p * 2.0);
            }
            return mix(mid, over(mid, b), p * 2.0 - 1.0);
        }
        // Wipes: the incoming clip is revealed from one side.
        case 3u: { return mix(a, b, reveal(1.0 - uv.x, p)); }
        case 4u: { return mix(a, b, reveal(uv.x, p)); }
        case 5u: { return mix(a, b, reveal(1.0 - uv.y, p)); }
        case 6u: { return mix(a, b, reveal(uv.y, p)); }
        // Slides: the incoming clip slides in over the outgoing one.
        case 7u: { return over(a, sb(uv - vec2(1.0 - p, 0.0))); }
        case 8u: { return over(a, sb(uv + vec2(1.0 - p, 0.0))); }
        case 9u: { return over(a, sb(uv - vec2(0.0, 1.0 - p))); }
        case 10u: { return over(a, sb(uv + vec2(0.0, 1.0 - p))); }
        // Push: both move left together.
        case 11u: {
            let e = smoothstep(0.0, 1.0, p);
            return over(sa(uv + vec2(e, 0.0)), sb(uv - vec2(1.0 - e, 0.0)));
        }
        // Zoom in: the outgoing clip grows and fades; the incoming one
        // settles from slightly large.
        case 12u: {
            let za = sa(0.5 + (uv - 0.5) / (1.0 + p));
            let zb = sb(0.5 + (uv - 0.5) / (1.0 + 0.3 * (1.0 - p)));
            return mix(za, zb, smoothstep(0.25, 0.75, p));
        }
        // Zoom out: the outgoing clip shrinks away; the incoming one grows
        // from slightly small.
        case 13u: {
            let za = sa(0.5 + (uv - 0.5) / max(1.0 - 0.5 * p, 0.05));
            let zb = sb(0.5 + (uv - 0.5) / (0.7 + 0.3 * p));
            return mix(za, zb, smoothstep(0.25, 0.75, p));
        }
        // Spin: out turning and shrinking, then in turning and growing.
        case 14u: {
            let d = (uv - 0.5) * vec2(aspect, 1.0);
            if p < 0.5 {
                let s = max(1.0 - p * 2.0, 1e-3);
                let q = rotate(d, p * TAU) / s / vec2(aspect, 1.0) + 0.5;
                return over(BLACK, sa(q));
            }
            let s = max(p * 2.0 - 1.0, 1e-3);
            let q = rotate(d, (p - 1.0) * TAU) / s / vec2(aspect, 1.0) + 0.5;
            return over(BLACK, sb(q));
        }
        // Circle reveal from the centre.
        case 15u: {
            let r = length((uv - 0.5) * vec2(aspect, 1.0)) / length(vec2(aspect, 1.0) * 0.5);
            return mix(a, b, reveal(r, p));
        }
        // Glitch: torn bands and split colour peaking at the cut.
        case 16u: {
            let strength = sin(p * 3.14159265);
            let band = u32(floor(uv.y * 18.0));
            let beat = u32(p * 20.0);
            var q = uv;
            if rand3(band, beat, 23u) < 0.5 * strength {
                q.x += (rand3(band, beat, 29u) - 0.5) * 0.2 * strength;
            }
            let off = vec2(0.015 * strength, 0.0);
            let use_b = p > 0.5 || rand3(band, beat, 31u) < p * strength;
            if use_b {
                let m = sb(q);
                return vec4(sb(q + off).r, m.g, sb(q - off).b, m.a);
            }
            let m = sa(q);
            return vec4(sa(q + off).r, m.g, sa(q - off).b, m.a);
        }
        // Light leak: a warm glow sweeps across as the clips cross.
        case 17u: {
            let base = mix(a, b, smoothstep(0.3, 0.7, p));
            let centre = vec2(mix(-0.2, 1.2, p), 0.35);
            let d = length((uv - centre) * vec2(aspect, 1.0));
            let glow = exp(-d * d * 3.0) * sin(p * 3.14159265);
            let leak = vec3(1.0, 0.55, 0.25) * glow * 1.4;
            return vec4(base.rgb + leak * base.a, base.a);
        }
        // Blur dissolve: both blur towards the cut while crossing.
        case 18u: {
            let radius = sin(p * 3.14159265) * 0.03 * size.y;
            return mix(spiral(uv, radius, false), spiral(uv, radius, true), smoothstep(0.2, 0.8, p));
        }
        // Page turn: a straight fold sweeps from the bottom-right corner;
        // the turned part shows the page's back, lighter, with a shadow.
        case 19u: {
            let dir = normalize(vec2(-1.0, -0.6));
            let q = (uv - vec2(1.0, 1.0)) * vec2(aspect, 1.0);
            let reach = (aspect * 0.6 + 1.0) * 1.2;
            let line = p * reach;
            let d = dot(q, dir);
            if d < line {
                // Turned already: the next page, shadowed along the fold.
                let shadow = 1.0 - 0.35 * exp(-(line - d) * 12.0) * step(0.001, p);
                return vec4(b.rgb * shadow, b.a);
            }
            // The turned part lies folded over, mirrored across the fold,
            // showing the page's lighter back.
            let mirrored = q - dir * 2.0 * (d - line);
            let back_uv = mirrored / vec2(aspect, 1.0) + vec2(1.0, 1.0);
            if d < 2.0 * line && all(back_uv >= vec2(0.0)) && all(back_uv <= vec2(1.0)) {
                let flap = sa(back_uv);
                return over(a, vec4(mix(flap.rgb, vec3(flap.a), 0.55), flap.a));
            }
            return a;
        }
        default: { return mix(a, b, p); }
    }
}

// Blends a clip drawn on its own (`src_tex`) onto the canvas (`other_tex`)
// with blend mode `F.b.w`.
@fragment
fn fs_merge(in: VsOut) -> @location(0) vec4<f32> {
    let px = in.uv * F.a.xy;
    let dst = textureLoad(other_tex, vec2<i32>(floor(px)), 0);
    let src = textureLoad(src_tex, vec2<i32>(floor(px)), 0);
    if src.a <= 0.0 {
        return dst;
    }
    let s = src.rgb / src.a;
    let mode = u32(F.b.w + 0.5);
    var mixed = s;
    if mode != 0u {
        mixed = to_linear(blend(to_srgb(dst.rgb), to_srgb(s), mode));
    }
    return vec4(mix(dst.rgb, mixed, min(src.a, 1.0)), 1.0);
}
