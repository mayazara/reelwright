// Full-screen triangle and colour helpers shared by the compositor and
// effect shaders.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VsOut;
    out.pos = vec4(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2(p.x, 1.0 - p.y);
    return out;
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow(max((c + 0.055) / 1.055, vec3(0.0)), vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3(0.2126, 0.7152, 0.0722));
}

// Blend functions on sRGB-encoded values.
fn blend(d: vec3<f32>, s: vec3<f32>, mode: u32) -> vec3<f32> {
    switch mode {
        case 1u: { return d * s; }
        case 2u: { return 1.0 - (1.0 - d) * (1.0 - s); }
        case 3u: { return select(1.0 - 2.0 * (1.0 - d) * (1.0 - s), 2.0 * d * s, d < vec3(0.5)); }
        case 4u: { return min(d, s); }
        case 5u: { return max(d, s); }
        case 6u: { return select(min(vec3(1.0), d / max(1.0 - s, vec3(1e-5))), vec3(1.0), s >= vec3(1.0)); }
        case 7u: { return select(1.0 - min(vec3(1.0), (1.0 - d) / max(s, vec3(1e-5))), vec3(0.0), s <= vec3(0.0)); }
        case 8u: { return min(vec3(1.0), d + s); }
        case 9u: { return abs(d - s); }
        default: { return s; }
    }
}
