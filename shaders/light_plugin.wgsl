// LightMaterial fragment shader (WGSL).
//
// This file is the source of truth for the LightPlugin's fragment pass and is
// loaded by `light_plugin.rs` via `Shader::from_wgsl`. The GLSL companion
// (`light_plugin.glsl`) is kept around because its compute passes (SEED /
// JFA / DIST) reference the same module — those passes are dead code today
// but worth preserving as reference for a future SDF lighting path.
//
// Why WGSL instead of GLSL?
// Newer wgpu pipeline validation requires the fragment shader's input
// `@interpolate(perspective, center)` annotation to *exactly* match the
// vertex shader's output. Bevy's mesh2d vertex shader emits explicit
// `Some(Center)` sampling, while a GLSL fragment input compiles to
// `Some(None)` (no explicit decoration). That mismatch trips
// `transparent_mesh2d_pipeline` validation. WGSL with explicit
// `@interpolate` matches Bevy exactly.

// === BINDINGS ===
@group(2) @binding(0) var occlusion_tex: texture_2d<f32>;
@group(2) @binding(1) var occlusion_samp: sampler;

// Mirrors `LightUniform` in `light_plugin.rs`. `_pad0..2` are required for the
// 16-byte alignment that `bytemuck::Pod` enforces on the Rust side.
struct Params {
    light_uv: vec2<f32>,
    radius: f32,
    intensity: f32,
    color: vec4<f32>,
    occlusion_size: vec2<f32>,
    visibility: f32,
    debug_mode: f32,
    raymarch_steps: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};
@group(2) @binding(2) var<uniform> params: Params;

// === INPUT ===
//
// `@interpolate(perspective, center)` is required (see header note).
struct FsIn {
    @location(2) @interpolate(perspective, center) uv: vec2<f32>,
    @builtin(position) pos: vec4<f32>,
};

const OCCLUSION_THRESHOLD: f32 = 0.5;
const MAX_STEPS: i32 = 64;

fn raymarch(uv: vec2<f32>, light_uv: vec2<f32>, steps: i32) -> bool {
    let dir = (uv - light_uv) / f32(steps);
    var p = light_uv;
    for (var i: i32 = 0; i < MAX_STEPS; i = i + 1) {
        if (i >= steps) { break; }
        let occ = textureSample(occlusion_tex, occlusion_samp, p).r;
        if (occ > OCCLUSION_THRESHOLD) {
            return true;
        }
        p = p + dir;
    }
    return false;
}

@fragment
fn fragment(in: FsIn) -> @location(0) vec4<f32> {
    let uv = in.uv;

    // Debug overlays — match the GLSL ones one-for-one so toggling
    // `debug_mode` from gameplay produces the same picture as before.
    if (params.debug_mode > 3.5) {
        let center = vec2<f32>(0.5, 0.5);
        let d = distance(uv, center);
        let t = 1.0 - clamp(d * 2.0, 0.0, 1.0);
        return vec4<f32>(0.0, 0.0, t, 1.0);
    }
    if (params.debug_mode > 2.5) {
        return vec4<f32>(params.light_uv.x, params.light_uv.y, 0.0, 1.0);
    }
    if (params.debug_mode > 1.5) {
        return vec4<f32>(uv.x, uv.y, 0.0, 1.0);
    }
    if (params.debug_mode > 0.5) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }

    let to_px = params.occlusion_size;
    let diff = uv * to_px - params.light_uv * to_px;
    let d2 = dot(diff, diff);
    let radius2 = params.radius * params.radius;

    // Outside the light radius — render the dark overlay outright. This
    // short-circuit matches the GLSL fragment so the visual cutoff stays
    // identical.
    if (d2 > radius2) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.6);
    }

    let d = sqrt(d2);
    let steps = i32(max(params.raymarch_steps, 0.0));
    var blocked = false;
    if (steps > 0) {
        blocked = raymarch(uv, params.light_uv, steps);
    }

    var light = 0.0;
    if (!blocked) {
        let falloff = 1.0 - clamp(d / params.radius, 0.0, 1.0);
        light = smoothstep(0.0, 1.0, falloff) * params.intensity;
    }

    let light_factor = clamp(light * params.visibility, 0.0, 1.0);
    let darkness = 1.0 - light_factor;
    let alpha = darkness * 0.6;
    return vec4<f32>(0.0, 0.0, 0.0, alpha);
}
