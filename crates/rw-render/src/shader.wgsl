// Compositor shaders (`common.wgsl` is prepended).
//
// Each layer is one full-canvas pass: for every canvas pixel, find where it
// lands in the layer (inverse transform, crop, orientation), shade the layer
// (colour adjust, LUT), blend with the canvas underneath and mix by opacity.
// The canvas is linear light; colour grading and blend modes work on
// sRGB-encoded values, as consumer editors do.

struct Layer {
    // Box centre and half size, in output pixels.
    center: vec2<f32>,
    half_box: vec2<f32>,
    // Half size of the picture inside the box (differs from the box for
    // fit/fill), and cos/sin of the clockwise rotation.
    half_content: vec2<f32>,
    rot: vec2<f32>,
    // Crop: top-left and size of the kept area in display UV.
    crop_origin: vec2<f32>,
    crop_size: vec2<f32>,
    // ±1 per axis for flips; output size in pixels.
    flip: vec2<f32>,
    out_size: vec2<f32>,
    // x: source quarter turns, y: opacity, z: blend mode, w: source kind
    // (0 texture, 1 solid, 2 gradient).
    p0: vec4<f32>,
    // exposure, brightness, contrast, saturation
    c0: vec4<f32>,
    // vibrance, temperature, tint, highlights
    c1: vec4<f32>,
    // shadows, LUT intensity, LUT size, clip to box (1 for fill)
    c2: vec4<f32>,
    // Solid colour or gradient start / end (sRGB, straight alpha); the
    // gradient direction (xy) and whether the layer is drawn on its own (z,
    // premultiplied over transparency, blend mode left for later).
    fill0: vec4<f32>,
    fill1: vec4<f32>,
    grad: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var lin_sampler: sampler;
@group(0) @binding(2) var<uniform> L: Layer;
@group(0) @binding(3) var dst_tex: texture_2d<f32>;
@group(0) @binding(4) var lut_tex: texture_3d<f32>;

// Colour adjustments and filter. Input and output linear light.
fn grade(lin_in: vec3<f32>) -> vec3<f32> {
    var c = lin_in * exp2(L.c0.x);
    let temp = L.c1.y;
    let tint = L.c1.z;
    c = c * vec3(1.0 + 0.2 * temp + 0.1 * tint, 1.0 - 0.2 * tint, 1.0 - 0.2 * temp + 0.1 * tint);
    var s = clamp(to_srgb(c), vec3(0.0), vec3(1.0));
    s = s + L.c0.y * 0.25;
    s = (s - 0.5) * (1.0 + L.c0.z) + 0.5;
    let l = luma(s);
    s = s + L.c2.x * 0.3 * (1.0 - smoothstep(0.0, 0.55, l)) + L.c1.w * 0.3 * smoothstep(0.45, 1.0, l);
    let l2 = luma(s);
    s = mix(vec3(l2), s, 1.0 + L.c0.w);
    let spread = max(max(s.r, s.g), s.b) - min(min(s.r, s.g), s.b);
    s = mix(vec3(l2), s, 1.0 + L.c1.x * (1.0 - clamp(spread, 0.0, 1.0)));
    s = clamp(s, vec3(0.0), vec3(1.0));
    if L.c2.y > 0.0 {
        let n = L.c2.z;
        let coord = s * ((n - 1.0) / n) + 0.5 / n;
        let graded = textureSampleLevel(lut_tex, lin_sampler, coord, 0.0).rgb;
        s = mix(s, graded, L.c2.y);
    }
    return to_linear(s);
}

// Straight-alpha sRGB colour of the layer at display UV `uv`.
fn source(uv: vec2<f32>) -> vec4<f32> {
    let kind = u32(L.p0.w + 0.5);
    if kind == 1u {
        return vec4(to_linear(L.fill0.rgb), L.fill0.a);
    }
    if kind == 2u {
        let dir = L.grad.xy;
        let span = abs(dir.x) * 0.5 + abs(dir.y) * 0.5;
        let t = clamp(dot(uv - 0.5, dir) / max(span, 1e-5) * 0.5 + 0.5, 0.0, 1.0);
        let c = mix(L.fill0, L.fill1, t);
        return vec4(to_linear(c.rgb), c.a);
    }
    // Map display UV back to the texture's stored orientation.
    let turns = u32(L.p0.x + 0.5) % 4u;
    var t = uv;
    if turns == 1u { t = vec2(uv.y, 1.0 - uv.x); }
    if turns == 2u { t = vec2(1.0 - uv.x, 1.0 - uv.y); }
    if turns == 3u { t = vec2(1.0 - uv.y, uv.x); }
    return textureSampleLevel(src_tex, lin_sampler, t, 0.0);
}

@fragment
fn fs_layer(in: VsOut) -> @location(0) vec4<f32> {
    let px = in.uv * L.out_size;
    let isolated = L.grad.z > 0.5;
    var dst = vec4(0.0);
    if !isolated {
        dst = textureLoad(dst_tex, vec2<i32>(floor(px)), 0);
    }
    // Into the box's frame: undo the clockwise rotation.
    let d = px - L.center;
    let local = vec2(L.rot.x * d.x + L.rot.y * d.y, -L.rot.y * d.x + L.rot.x * d.y);
    // Coverage with a one-pixel soft edge, against the picture or (for
    // fill) the box.
    var half_clip = L.half_content;
    if L.c2.w > 0.5 {
        half_clip = min(L.half_box, L.half_content);
    }
    let edge = half_clip - abs(local);
    let coverage = clamp(min(edge.x, edge.y) + 0.5, 0.0, 1.0);
    if coverage <= 0.0 {
        return dst;
    }
    var uv = local / (2.0 * L.half_content) * L.flip + 0.5;
    uv = L.crop_origin + clamp(uv, vec2(0.0), vec2(1.0)) * L.crop_size;
    let src = source(uv);
    let a = src.a * L.p0.y * coverage;
    if a <= 0.0 {
        return dst;
    }
    let graded = grade(src.rgb);
    if isolated {
        return vec4(graded * a, a);
    }
    let mode = u32(L.p0.z + 0.5);
    var mixed = graded;
    if mode != 0u {
        mixed = to_linear(blend(to_srgb(dst.rgb), to_srgb(graded), mode));
    }
    return vec4(mix(dst.rgb, mixed, a), 1.0);
}

@fragment
fn fs_blit(in: VsOut) -> @location(0) vec4<f32> {
    let px = in.uv * L.out_size;
    let c = textureLoad(dst_tex, vec2<i32>(floor(px)), 0);
    return vec4(c.rgb, 1.0);
}
